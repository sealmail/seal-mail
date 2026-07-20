use crate::crypto::{restrict_perms, Identity};
use crate::models::*;
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

pub struct AppState {
    pub inner: Mutex<StoreData>,
}

impl AppState {
    /// 取全局状态锁；若曾有线程持锁 panic，恢复内部数据而不是永久毒化。
    pub fn lock(&self) -> MutexGuard<'_, StoreData> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
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
    /// 用户隐藏的服务器目录（IMAP 服务器拒绝删除的内置目录仍可从侧栏移除）
    pub hidden_folders: HashMap<String, Vec<String>>,
    /// POP3 邮件 → 本地目录的归属（key: account_id/uid）
    pub local_assign: HashMap<String, String>,
    /// POP3 已读标记（key: account_id/uid）
    pub local_read: Vec<String>,
    /// 自动收集的联系人（key: 小写邮箱）
    pub contacts: HashMap<String, Contact>,
    /// 写信草稿（本地）
    pub drafts: Vec<Draft>,
    /// 内存缓存：完整邮件，key = account/folder/uid
    pub mail_cache: HashMap<String, EmailFull>,
    /// SQLite 邮件缓存（mail.db；原始邮件 + 列表状态，离线可读）
    pub db: rusqlite::Connection,
}

/// 文件不存在 → 默认值（首次启动）。
/// 文件存在但 JSON 损坏 → 报错，并把坏文件改名为 `*.corrupt.<ts>`，避免后续 save 用空表覆盖真数据。
fn read_json<T: DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    match fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(format!("读取 {} 失败: {e}", path.display())),
        Ok(raw) => {
            if raw.trim().is_empty() {
                return Ok(T::default());
            }
            match serde_json::from_str(&raw) {
                Ok(v) => Ok(v),
                Err(e) => {
                    let label = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("配置文件");
                    let backup = corrupt_backup_path(path);
                    match fs::rename(path, &backup) {
                        Ok(()) => {
                            let backup_name = backup
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("*.corrupt");
                            Err(format!("解析 {label} 失败（已备份为 {backup_name}）: {e}"))
                        }
                        Err(rename_err) => Err(format!(
                            "解析 {label} 失败: {e}（备份失败: {rename_err}）"
                        )),
                    }
                }
            }
        }
    }
}

fn corrupt_backup_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    path.with_file_name(format!("{name}.corrupt.{ts}"))
}

/// 原子落盘：写临时文件 → fsync → rename，避免写一半断电把配置截断成损坏 JSON。
fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let s = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("无效路径: {}", path.display()))?;
    let tmp_path = path.with_file_name(format!("{file_name}.tmp"));
    {
        let mut f = File::create(&tmp_path)
            .map_err(|e| format!("写入 {} 失败: {e}", tmp_path.display()))?;
        f.write_all(s.as_bytes())
            .map_err(|e| format!("写入 {} 失败: {e}", tmp_path.display()))?;
        f.sync_all()
            .map_err(|e| format!("fsync {} 失败: {e}", tmp_path.display()))?;
    }
    fs::rename(&tmp_path, path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        format!("替换 {} 失败: {e}", path.display())
    })?;
    if let Some(parent) = path.parent() {
        if let Ok(dirf) = File::open(parent) {
            let _ = dirf.sync_all();
        }
    }
    Ok(())
}

impl StoreData {
    pub fn load(dir: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let identity = crate::crypto::load_or_create_identity(&dir)?;
        let db = crate::db::open(&dir)?;
        let prefs: AppPrefs = read_json(&dir.join("prefs.json"))?;
        // 用户可见文案（错误/通知）的语言全局：加载偏好时同步
        crate::i18n::set_lang_from_pref(&prefs.language);
        Ok(StoreData {
            db,
            accounts: read_json(&dir.join("accounts.json"))?,
            secrets: read_json(&dir.join("secrets.json"))?,
            filters: read_json(&dir.join("filters.json"))?,
            trusted: read_json(&dir.join("trusted.json"))?,
            local_folders: read_json(&dir.join("local_folders.json"))?,
            hidden_folders: read_json(&dir.join("hidden_folders.json"))?,
            local_assign: read_json(&dir.join("local_assign.json"))?,
            local_read: read_json(&dir.join("local_read.json"))?,
            contacts: read_json(&dir.join("contacts.json"))?,
            drafts: read_json(&dir.join("drafts.json"))?,
            identity_config: read_json(&dir.join("identity.json"))?,
            prefs,
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

    /// 只重读偏好（GUI 常驻进程在 CLI 子进程改完 prefs 后刷新内存态用）
    pub fn load_prefs(dir: &Path) -> Result<AppPrefs, String> {
        read_json(&dir.join("prefs.json"))
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

    /// CLI 子进程改完账户/凭据后，GUI 常驻进程重读磁盘并更新内存。
    /// 否则新账户没有 watcher、已删账户仍被旧凭据轮询。
    pub fn reload_accounts_and_secrets_from_disk(&mut self) -> Result<(), String> {
        self.accounts = read_json(&self.dir.join("accounts.json"))?;
        self.secrets = read_json(&self.dir.join("secrets.json"))?;
        Ok(())
    }

    /// 合并更新单个账户凭据。GUI 常驻进程的内存状态可能落后于 CLI 子进程；
    /// 写入前必须重读磁盘，避免 OAuth 刷新把后来新增的账户凭据整表覆盖掉。
    pub fn update_secret(&mut self, account_id: &str, secret: AccountSecret) -> Result<(), String> {
        let p = self.dir.join("secrets.json");
        let raw = fs::read_to_string(&p).map_err(|e| format!("读取账户凭据失败: {e}"))?;
        let mut latest: HashMap<String, AccountSecret> =
            serde_json::from_str(&raw).map_err(|e| format!("解析账户凭据失败: {e}"))?;
        latest.insert(account_id.to_string(), secret);
        write_json(&p, &latest)?;
        restrict_perms(&p);
        self.secrets = latest;
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

    pub fn save_drafts(&self) -> Result<(), String> {
        write_json(&self.dir.join("drafts.json"), &self.drafts)
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

    pub fn save_hidden_folders(&self) -> Result<(), String> {
        write_json(&self.dir.join("hidden_folders.json"), &self.hidden_folders)
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
