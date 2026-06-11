use crate::crypto::{restrict_perms, Identity};
use crate::models::*;
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct AppState {
    pub inner: Mutex<StoreData>,
}

pub struct StoreData {
    pub dir: PathBuf,
    pub identity: Identity,
    pub identity_config: IdentityConfig,
    pub accounts: Vec<Account>,
    pub secrets: HashMap<String, AccountSecret>,
    pub filters: Vec<FilterRule>,
    pub trusted: Vec<TrustedContact>,
    /// 本地虚拟目录（POP3 账户没有服务器目录时使用；IMAP 账户用服务器目录）
    pub local_folders: Vec<String>,
    /// POP3 邮件 → 本地目录的归属（key: account_id/uid）
    pub local_assign: HashMap<String, String>,
    /// POP3 已读标记（key: account_id/uid）
    pub local_read: Vec<String>,
    /// 内存缓存：完整邮件，key = account/folder/uid
    pub mail_cache: HashMap<String, EmailFull>,
}

fn read_json<T: DeserializeOwned + Default>(path: &PathBuf) -> T {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_json<T: Serialize>(path: &PathBuf, value: &T) -> Result<(), String> {
    let s = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    fs::write(path, s).map_err(|e| e.to_string())
}

impl StoreData {
    pub fn load(dir: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let identity = crate::crypto::load_or_create_identity(&dir)?;
        Ok(StoreData {
            accounts: read_json(&dir.join("accounts.json")),
            secrets: read_json(&dir.join("secrets.json")),
            filters: read_json(&dir.join("filters.json")),
            trusted: read_json(&dir.join("trusted.json")),
            local_folders: read_json(&dir.join("local_folders.json")),
            local_assign: read_json(&dir.join("local_assign.json")),
            local_read: read_json(&dir.join("local_read.json")),
            identity_config: read_json(&dir.join("identity.json")),
            mail_cache: HashMap::new(),
            identity,
            dir,
        })
    }

    pub fn save_identity_config(&self) -> Result<(), String> {
        write_json(&self.dir.join("identity.json"), &self.identity_config)
    }

    /// 当前生效的签名身份标识（本地=Ed25519 指纹，Ledger=0x 地址）
    pub fn active_fingerprint(&self) -> String {
        if self.identity_config.mode == "ledger" {
            self.identity_config
                .ledger_address
                .clone()
                .unwrap_or_else(|| self.identity.fingerprint())
        } else {
            self.identity.fingerprint()
        }
    }

    pub fn save_accounts(&self) -> Result<(), String> {
        write_json(&self.dir.join("accounts.json"), &self.accounts)
    }
    pub fn save_secrets(&self) -> Result<(), String> {
        let p = self.dir.join("secrets.json");
        write_json(&p, &self.secrets)?;
        restrict_perms(&p);
        Ok(())
    }
    pub fn save_filters(&self) -> Result<(), String> {
        write_json(&self.dir.join("filters.json"), &self.filters)
    }
    pub fn save_trusted(&self) -> Result<(), String> {
        write_json(&self.dir.join("trusted.json"), &self.trusted)
    }
    pub fn save_local_folders(&self) -> Result<(), String> {
        write_json(&self.dir.join("local_folders.json"), &self.local_folders)?;
        write_json(&self.dir.join("local_assign.json"), &self.local_assign)?;
        write_json(&self.dir.join("local_read.json"), &self.local_read)
    }

    pub fn account(&self, id: &str) -> Result<Account, String> {
        self.accounts
            .iter()
            .find(|a| a.id == id)
            .cloned()
            .ok_or_else(|| format!("账户不存在: {}", id))
    }
    pub fn secret(&self, id: &str) -> Result<AccountSecret, String> {
        self.secrets
            .get(id)
            .cloned()
            .ok_or_else(|| format!("账户密码缺失: {}", id))
    }

    pub fn cache_key(account: &str, folder: &str, uid: u32) -> String {
        format!("{}/{}/{}", account, folder, uid)
    }
}
