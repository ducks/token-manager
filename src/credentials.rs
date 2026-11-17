use crate::state::{Account, Credential, read_private};
use anyhow::{Result, bail, ensure};
use std::{
    io::Write,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub fn validate(secret: String) -> Result<String> {
    let secret = secret.trim().to_owned();
    ensure!(
        !secret.is_empty() && secret.len() <= 8192 && !secret.chars().any(char::is_control),
        "credential must be nonempty, bounded and contain no control characters"
    );
    Ok(secret)
}
fn keyring(action: &str, name: &str, input: Option<&str>) -> Result<String> {
    let mut command = Command::new("secret-tool");
    command.arg(action);
    if action == "store" {
        command.arg("--label=Token Manager account credential");
    }
    command
        .args(["application", "token-manager", "account", name])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().map_err(|_| {
        anyhow::anyhow!("OS keyring unavailable; install secret-tool, or use --env or --key-file")
    })?;
    if let Some(input) = input {
        child.stdin.take().unwrap().write_all(input.as_bytes())?;
    } else {
        drop(child.stdin.take());
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("OS keyring did not respond; unlock it or use --env/--key-file");
        }
        thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output()?;
    ensure!(
        output.status.success(),
        "OS keyring operation failed; unlock your keyring or use --env/--key-file"
    );
    String::from_utf8(output.stdout).map_err(|_| anyhow::anyhow!("invalid credential encoding"))
}
pub fn store(name: &str, secret: &str) -> Result<()> {
    keyring("store", name, Some(secret))?;
    Ok(())
}
pub fn resolve(account: &Account) -> Result<String> {
    let value = match &account.credential {
        Credential::None => bail!("No credential method for this account capability"),
        Credential::Keyring(id) => keyring("lookup", id, None)?,
        Credential::Env(name) => std::env::var(name)
            .map_err(|_| anyhow::anyhow!("credential environment variable is unset"))?,
        Credential::File(path) => String::from_utf8(read_private(path)?)
            .map_err(|_| anyhow::anyhow!("invalid credential encoding"))?,
    };
    validate(value)
}
