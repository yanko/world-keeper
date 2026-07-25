//! Password storage via the OS keyring (Windows Credential Manager on the dev
//! machine). Config files (`config.rs`) never see the secret itself — only a
//! `savePassword` flag and the profile id used as the keyring username.

use keyring::Entry;

const SERVICE: &str = "com.yanko.ck-world-manager";

fn entry(profile_id: &str) -> anyhow::Result<Entry> {
    Ok(Entry::new(SERVICE, profile_id)?)
}

pub fn save_password(profile_id: &str, password: &str) -> anyhow::Result<()> {
    entry(profile_id)?.set_password(password)?;
    Ok(())
}

pub fn get_password(profile_id: &str) -> anyhow::Result<Option<String>> {
    match entry(profile_id)?.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn delete_password(profile_id: &str) -> anyhow::Result<()> {
    match entry(profile_id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
