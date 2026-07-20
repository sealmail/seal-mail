use crate::crypto::Identity;
use crate::models::*;
use crate::secrets_store;
use serde::{de::DeserializeOwned, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

/// 完整邮件内存缓存上限（LRU）
const MAIL_CACHE_CAP: usize = 256;

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
    /// 内存缓存：完整邮件，key = account/folder/uid（LRU，见 MAIL_CACHE_CAP）
    pub mail_cache: HashMap<String, EmailFull>,
    /// LRU 顺序：队尾最近访问
    mail_cache_order: VecDeque<String>,
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
            // 空文件不是合法状态:write_json 永远写完整 JSON,空内容意味着写入被截断。
            // 静默当默认值会把仅存的备份线索(原文件)覆盖掉。
            let parsed = if raw.trim().is_empty() {
                Err(serde_json::Error::io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "文件为空,疑似写入被截断",
                )))
            } else {
                serde_json::from_str(&raw)
            };
            match parsed {
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

/// 非关键本地辅助数据损坏时，read_json 已先备份原文件；记录后用默认值继续启动。
/// accounts / secrets / trusted / identity 等身份与安全数据仍走硬失败。
fn read_json_recoverable<T: DeserializeOwned + Default>(path: &Path) -> T {
    match read_json(path) {
        Ok(value) => value,
        Err(e) => {
            crate::logging::log(format!("[store] {e}; 已用默认值恢复启动"));
            T::default()
        }
    }
}

fn corrupt_backup_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    // 纳秒时间戳：同一文件一秒内两次损坏时备份名不能互相覆盖
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    path.with_file_name(format!("{name}.corrupt.{ts}"))
}

/// 原子落盘：写临时文件 → fsync → rename，避免写一半断电把配置截断成损坏 JSON。
/// 临时文件名含 pid + 进程内序号：GUI 与 CLI 子进程并发保存同一文件时,
/// 固定 tmp 名会互相截断对方写到一半的内容,rename 后把半截字节顶进正式文件。
pub(crate) fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("无效路径: {}", path.display()))?;
    let tmp_path = path.with_file_name(format!(
        "{file_name}.{}.{}.tmp",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let write_result = (|| {
        let mut f = File::create(&tmp_path)
            .map_err(|e| format!("写入 {} 失败: {e}", tmp_path.display()))?;
        // 秘密文件的权限要在写入内容前收紧,rename 之后再 chmod 会留下可读窗口
        crate::crypto::restrict_perms(&tmp_path);
        f.write_all(bytes)
            .map_err(|e| format!("写入 {} 失败: {e}", tmp_path.display()))?;
        f.sync_all()
            .map_err(|e| format!("fsync {} 失败: {e}", tmp_path.display()))?;
        fs::rename(&tmp_path, path).map_err(|e| format!("替换 {} 失败: {e}", path.display()))
    })();
    if let Err(e) = write_result {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }
    if let Some(parent) = path.parent() {
        if let Ok(dirf) = File::open(parent) {
            let _ = dirf.sync_all();
        }
    }
    Ok(())
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let s = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    atomic_write_bytes(path, s.as_bytes())
}

/// 跨进程文件锁的有界等待：try_lock + 睡眠重试，约 10 秒拿不到就报错。
/// 锁内可能做钥匙串 I/O（卡在系统授权弹窗），lock_exclusive 无限等待会把
/// 其它进程（含持 AppState 锁的 GUI）全部冻住。
pub(crate) fn lock_exclusive_bounded(file: &File, busy: &str) -> Result<(), String> {
    use fs2::FileExt;
    let start = std::time::Instant::now();
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(()),
            Err(ref e) if e.kind() == fs2::lock_contended_error().kind() => {
                if start.elapsed() >= std::time::Duration::from_secs(10) {
                    return Err(busy.to_string());
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => return Err(format!("获取文件锁失败: {e}")),
        }
    }
}

/// 在 accounts.lock 跨进程锁内读-改-写 accounts.json（同 secrets_store::update_account）。
/// GUI 常驻进程与 CLI 子进程并发增删账户时，各自拿旧表整表写回会互相覆盖（丢更新）。
/// 返回落盘后的最新账户表，调用方用它刷新内存态。
pub fn update_accounts_with(
    dir: &Path,
    mutate: impl FnOnce(&mut Vec<Account>),
) -> Result<Vec<Account>, String> {
    fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
    let lock_path = dir.join("accounts.lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&lock_path)
        .map_err(|e| format!("打开账户锁失败: {e}"))?;
    lock_exclusive_bounded(&lock, "账户配置正被另一进程更新，请稍后再试")?;

    let result = (|| {
        let mut latest: Vec<Account> = read_json(&dir.join("accounts.json"))?;
        mutate(&mut latest);
        write_json(&dir.join("accounts.json"), &latest)?;
        Ok(latest)
    })();
    let _ = lock.unlock();
    result
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
            secrets: secrets_store::load(&dir)?,
            filters: read_json_recoverable(&dir.join("filters.json")),
            trusted: read_json(&dir.join("trusted.json"))?,
            local_folders: read_json_recoverable(&dir.join("local_folders.json")),
            hidden_folders: read_json_recoverable(&dir.join("hidden_folders.json")),
            local_assign: read_json_recoverable(&dir.join("local_assign.json")),
            local_read: read_json_recoverable(&dir.join("local_read.json")),
            contacts: read_json_recoverable(&dir.join("contacts.json")),
            drafts: read_json_recoverable(&dir.join("drafts.json")),
            identity_config: read_json(&dir.join("identity.json"))?,
            prefs,
            mail_cache: HashMap::new(),
            mail_cache_order: VecDeque::new(),
            identity,
            dir,
        })
    }

    pub fn cache_get(&mut self, key: &str) -> Option<&EmailFull> {
        if self.mail_cache.contains_key(key) {
            self.mail_cache_order.retain(|k| k != key);
            self.mail_cache_order.push_back(key.to_string());
            self.mail_cache.get(key)
        } else {
            None
        }
    }

    pub fn cache_put(&mut self, key: String, full: EmailFull) {
        if self.mail_cache.contains_key(&key) {
            self.mail_cache_order.retain(|k| k != &key);
        }
        self.mail_cache.insert(key.clone(), full);
        self.mail_cache_order.push_back(key);
        while self.mail_cache_order.len() > MAIL_CACHE_CAP {
            if let Some(old) = self.mail_cache_order.pop_front() {
                self.mail_cache.remove(&old);
            } else {
                break;
            }
        }
    }

    pub fn cache_remove(&mut self, key: &str) -> Option<EmailFull> {
        self.mail_cache_order.retain(|k| k != key);
        self.mail_cache.remove(key)
    }

    pub fn cache_clear(&mut self) {
        self.mail_cache.clear();
        self.mail_cache_order.clear();
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
        secrets_store::save(&self.dir, &self.secrets)
    }

    /// CLI 子进程改完账户/凭据后，GUI 常驻进程重读磁盘并更新内存。
    /// 否则新账户没有 watcher、已删账户仍被旧凭据轮询。
    pub fn reload_accounts_and_secrets_from_disk(&mut self) -> Result<(), String> {
        self.accounts = read_json(&self.dir.join("accounts.json"))?;
        self.secrets = secrets_store::load(&self.dir)?;
        Ok(())
    }

    /// 合并更新单个账户凭据。GUI 常驻进程的内存状态可能落后于 CLI 子进程；
    /// 写入前必须重读磁盘，避免 OAuth 刷新把后来新增的账户凭据整表覆盖掉。
    pub fn update_secret(&mut self, account_id: &str, secret: AccountSecret) -> Result<(), String> {
        self.secrets = secrets_store::update_account(&self.dir, account_id, Some(secret))?;
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
