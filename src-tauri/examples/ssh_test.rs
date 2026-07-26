//! Manual, no-UI connectivity check: password-connect over SSH, run one `exec`, then the
//! discovery probe, and print everything. Run this yourself so the password never has to
//! be typed into chat:
//!
//!   cd src-tauri
//!   $env:CK_SSH_PASSWORD = "..."   # PowerShell; avoids shell history
//!   cargo run --example ssh_test
//!
//! Optional overrides: CK_SSH_HOST (default 192.168.1.50), CK_SSH_USER (default root),
//! CK_SSH_PORT (default 22).

use ck_world_manager_lib::discovery;
use ck_world_manager_lib::ssh::SshSession;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let host = std::env::var("CK_SSH_HOST").unwrap_or_else(|_| "192.168.1.50".to_string());
    let user = std::env::var("CK_SSH_USER").unwrap_or_else(|_| "root".to_string());
    let port: u16 = std::env::var("CK_SSH_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(22);
    let password = std::env::var("CK_SSH_PASSWORD").unwrap_or_else(|_| {
        std::env::args()
            .nth(1)
            .expect("set CK_SSH_PASSWORD, or pass the password as the first argument")
    });

    println!("Connecting to {user}@{host}:{port} ...");
    let session = SshSession::connect_password(&host, port, &user, &password).await?;
    println!("Connected.\n");

    println!("--- exec test ---");
    let out = session
        .exec("echo hello from $(hostname); uname -a")
        .await?;
    println!("exit status: {}", out.exit_status);
    print!("stdout:\n{}", out.stdout);
    if !out.stderr.is_empty() {
        print!("stderr:\n{}", out.stderr);
    }

    println!("\n--- discovery probe ---");
    let result = discovery::probe(&session).await?;
    println!("{result:#?}");

    Ok(())
}
