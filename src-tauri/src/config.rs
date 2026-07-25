//! Connection profile config (SPEC.md §5), persisted via `tauri-plugin-store` as plain
//! JSON in the app-config dir. Passwords never live here — see `secrets.rs`.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

use crate::target::ServiceKind;

const STORE_FILE: &str = "config.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethod {
    Key,
    Password,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Discovered {
    pub os: Option<String>,
    pub service_kind: Option<ServiceKind>,
    pub worlds_path: Option<String>,
    pub steam_user: Option<String>,
    pub launch_script: Option<String>,
    pub probed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub user: String,
    pub auth: AuthMethod,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_path: Option<String>,
    #[serde(default)]
    pub save_password: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discovered: Option<Discovered>,
}

fn default_port() -> u16 {
    22
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldDetection {
    pub patterns: Vec<String>,
}

impl Default for WorldDetection {
    fn default() -> Self {
        Self {
            patterns: vec!["*.world.gzip".to_string()],
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    #[serde(default)]
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub world_detection: WorldDetection,
    pub active_profile_id: Option<String>,
}

pub fn load<R: Runtime>(app: &AppHandle<R>) -> anyhow::Result<AppConfig> {
    let store = app.store(STORE_FILE)?;
    let Some(value) = store.get("config") else {
        return Ok(AppConfig::default());
    };
    Ok(serde_json::from_value(value)?)
}

pub fn save<R: Runtime>(app: &AppHandle<R>, config: &AppConfig) -> anyhow::Result<()> {
    let store = app.store(STORE_FILE)?;
    store.set("config", serde_json::to_value(config)?);
    store.save()?;
    Ok(())
}

pub fn upsert_profile<R: Runtime>(app: &AppHandle<R>, profile: Profile) -> anyhow::Result<AppConfig> {
    let mut config = load(app)?;
    if let Some(existing) = config.profiles.iter_mut().find(|p| p.id == profile.id) {
        *existing = profile;
    } else {
        config.profiles.push(profile);
    }
    save(app, &config)?;
    Ok(config)
}

pub fn delete_profile<R: Runtime>(app: &AppHandle<R>, profile_id: &str) -> anyhow::Result<AppConfig> {
    let mut config = load(app)?;
    config.profiles.retain(|p| p.id != profile_id);
    if config.active_profile_id.as_deref() == Some(profile_id) {
        config.active_profile_id = None;
    }
    save(app, &config)?;
    Ok(config)
}

pub fn set_discovered<R: Runtime>(
    app: &AppHandle<R>,
    profile_id: &str,
    discovered: Discovered,
) -> anyhow::Result<AppConfig> {
    let mut config = load(app)?;
    let profile = config
        .profiles
        .iter_mut()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| anyhow::anyhow!("no such profile: {profile_id}"))?;
    profile.discovered = Some(discovered);
    save(app, &config)?;
    Ok(config)
}
