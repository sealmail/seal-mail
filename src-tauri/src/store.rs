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
    pub prefs: AppPrefs,
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
    /// 自动收集的联系人（key: 小写邮箱）
    pub contacts: HashMap<String, Contact>,
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
            contacts: read_json(&dir.join("contacts.json")),
            identity_config: read_json(&dir.join("identity.json")),
            prefs: read_json(&dir.join("prefs.json")),
            mail_cache: HashMap::new(),
            identity,
            dir,
        })
    }

    pub fn save_identity_config(&self) -> Result<(), String> {
        write_json(&self.dir.join("identity.json"), &self.identity_config)
    }

    pub fn save_prefs(&self) -> Result<(), String> {
        write_json(&self.dir.join("prefs.json"), &self.prefs)
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
    pub fn save_contacts(&self) -> Result<(), String> {
        write_json(&self.dir.join("contacts.json"), &self.contacts)
    }

    /// 收/发邮件时静默收集联系人（自动补全用）。返回是否有变更（决定要不要落盘）。
    pub fn upsert_contact(&mut self, name: &str, email: &str, ts: i64) -> bool {
        let email = email.trim();
        if email.is_empty() || !email.contains('@') {
            return false;
        }
        let key = email.to_lowercase();
        let entry = self.contacts.entry(key).or_insert_with(|| Contact {
            name: String::new(),
            email: email.to_string(),
            last_seen: 0,
            count: 0,
        });
        entry.count += 1;
        if ts > entry.last_seen {
            entry.last_seen = ts;
        }
        if !name.trim().is_empty() && name.trim() != entry.email {
            entry.name = name.trim().to_string();
        }
        true
    }

    pub fn save_local_folders(&self) -> Result<(), String> {
        write_json(&self.dir.join("local_folders.json"), &self.local_folders)?;
        write_json(&self.dir.join("local_assign.json"), &self.local_assign)?;
        write_json(&self.dir.join("local_read.json"), &self.local_read)
    }

    /// 校验邮件用的可信列表：附加本机签名身份——
    /// 自己（含自己的其他设备同私钥）签发的邮件直接显示绿色「已验证」，
    /// 而不是黄色「签名有效·尚未列入可信」。
    pub fn trusted_for_verify(&self, account: &Account) -> Vec<TrustedContact> {
        let mut list = self.trusted.clone();
        list.push(TrustedContact {
            name: format!("{}（本人）", account.display_name),
            email: account.email.clone(),
            fingerprint: self.active_fingerprint(),
            org: None,
            since: self.identity.created.clone(),
            verified_count: 0,
        });
        list
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
