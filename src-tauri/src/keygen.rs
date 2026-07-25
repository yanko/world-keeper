//! Local SSH keypair generation for the "one-time setup helper" (SPEC.md §6): shells
//! out to the system `ssh-keygen` rather than generating in-process. Windows 10/11 ship
//! the OpenSSH client (and `ssh-keygen`) out of the box, and this sidesteps chasing
//! exact `rand_core`/`CryptoRng` version alignment with whatever `ssh-key` needs — the
//! well-trodden tool for this job is already sitting on PATH.
//!
//! Keys are written under the app's own data dir (`keys/<profile_id>_ed25519`), never
//! into the user's real `~/.ssh/` — this is a private key just for this app/profile.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use tauri::{AppHandle, Manager, Runtime};

pub struct GeneratedKey {
    pub private_key_path: PathBuf,
    pub public_key_line: String,
}

pub async fn generate_for_profile<R: Runtime>(
    app: &AppHandle<R>,
    profile_id: &str,
    comment: &str,
) -> anyhow::Result<GeneratedKey> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!("resolving app data dir: {e}"))?
        .join("keys");
    tokio::fs::create_dir_all(&dir)
        .await
        .with_context(|| format!("creating {}", dir.display()))?;

    let key_path = dir.join(format!("{profile_id}_ed25519"));
    if tokio::fs::try_exists(&key_path).await.unwrap_or(false) {
        bail!(
            "a key already exists at {} — delete it first to regenerate",
            key_path.display()
        );
    }

    let output = tokio::process::Command::new("ssh-keygen")
        .arg("-t")
        .arg("ed25519")
        .arg("-f")
        .arg(&key_path)
        .arg("-N")
        .arg("") // no passphrase — the private key file itself is the secret
        .arg("-C")
        .arg(comment)
        .arg("-q")
        .output()
        .await
        .context("running ssh-keygen — is the Windows OpenSSH Client feature installed?")?;

    if !output.status.success() {
        bail!(
            "ssh-keygen failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let pub_path = public_key_path(&key_path);
    let public_key_line = tokio::fs::read_to_string(&pub_path)
        .await
        .with_context(|| format!("reading {}", pub_path.display()))?
        .trim()
        .to_string();

    Ok(GeneratedKey {
        private_key_path: key_path,
        public_key_line,
    })
}

fn public_key_path(private_key_path: &Path) -> PathBuf {
    let mut s = private_key_path.as_os_str().to_owned();
    s.push(".pub");
    PathBuf::from(s)
}
