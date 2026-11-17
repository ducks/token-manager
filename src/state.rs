use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

static WRITE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "reference", rename_all = "snake_case")]
pub enum Credential {
    None,
    Keyring(String),
    Env(String),
    File(PathBuf),
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Account {
    pub name: String,
    pub provider: String,
    pub provider_fingerprint: String,
    pub credential: Credential,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub accounts: Vec<Account>,
}
pub fn private_directory(path: &Path) -> Result<()> {
    ensure!(
        !path.is_symlink(),
        "private directory must not be a symlink"
    );
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
pub fn read_private(path: &Path) -> Result<Vec<u8>> {
    ensure!(!path.is_symlink(), "private files must not be symlinks");
    let meta = path.metadata()?;
    ensure!(
        meta.is_file() && meta.len() <= 1048576 && meta.permissions().mode() & 0o077 == 0,
        "private file must be regular, bounded and mode 0600 or stricter"
    );
    Ok(fs::read(path)?)
}
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("missing parent directory"))?;
    private_directory(parent)?;
    ensure!(!path.is_symlink(), "refusing to replace symlink");
    let temporary = parent.join(format!(
        ".write-{}-{}",
        std::process::id(),
        WRITE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(data)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })()
}
pub fn load(path: &Path) -> Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }
    let config: Config = serde_json::from_slice(&read_private(path)?)
        .map_err(|_| anyhow::anyhow!("invalid account configuration"))?;
    ensure!(config.accounts.len() <= 64, "at most 64 accounts supported");
    let mut names = std::collections::HashSet::new();
    for account in &config.accounts {
        ensure!(
            crate::provider::identifier(&account.name)
                && crate::provider::identifier(&account.provider),
            "invalid account identity"
        );
        ensure!(names.insert(&account.name), "duplicate account name");
    }
    Ok(config)
}

pub struct EditLock(PathBuf);
impl EditLock {
    pub fn acquire(root: &Path) -> Result<Self> {
        let path = root.join("accounts.lock");
        let mut file = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path)
            .map_err(|_|anyhow::anyhow!("Account edit lock exists; finish the other edit, or inspect accounts.lock after an interrupted edit"))?;
        write!(file, "{}", std::process::id())?;
        Ok(Self(path))
    }
}
impl Drop for EditLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
