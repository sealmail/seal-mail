//! 账户凭据存储：优先系统钥匙串（macOS Keychain / Windows Credential Manager / secret-service），
//! 不可用时回退到 secrets.json（0600）。
//!
//! 钥匙串写入成功后，磁盘上只保留 `{"backend":"keychain"}` 占位标记，**不再**落盘明文密码。
//! 这样 CLI 与 GUI 同用户同配置目录仍可从钥匙串读到凭据；无钥匙串环境才用文件。
//!
//! 权威性规则：磁盘文件说了算。
//! - 文件是 marker → 数据在钥匙串,钥匙串读不出必须报错,绝不能静默变空表。
//! - 文件是明文 map → 它只会在钥匙串写失败时产生,必然不旧于钥匙串 → 以文件为准,
//!   并尝试迁回钥匙串(成功后 marker 覆盖明文;失败下次 load 重试)。
//!   若反过来让非空钥匙串优先,一次钥匙串故障期间更新的凭据会在故障恢复后被旧数据顶掉。

use crate::models::AccountSecret;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

const SERVICE: &str = "com.sealmail.app";

/// marker 文件必须只含 backend 字段:混入账户数据的文件绝不能被当成 marker 丢弃。
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MarkerFile {
    backend: String,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum SecretsFile {
    Map(HashMap<String, AccountSecret>),
    Marker(MarkerFile),
}

enum FileState {
    Missing,
    Marker,
    Map(HashMap<String, AccountSecret>),
}

/// 钥匙串后端抽象:测试用假实现,避免单测污染真实登录钥匙串。
pub(crate) trait KeychainBackend {
    /// Ok(None) = 钥匙串可用但没有本配置目录的条目。
    fn read(&self, dir: &Path) -> Result<Option<HashMap<String, AccountSecret>>, String>;
    fn write(&self, dir: &Path, map: &HashMap<String, AccountSecret>) -> Result<(), String>;
}

struct SystemKeychain;

/// 每个配置目录独立钥匙串条目，避免测试/多 profile 互踩。
fn entry_for(dir: &Path) -> Result<keyring::Entry, String> {
    use sha2::{Digest, Sha256};
    let canon = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let digest = Sha256::digest(canon.to_string_lossy().as_bytes());
    let name = format!("secrets-{}", hex::encode(&digest[..8]));
    keyring::Entry::new(SERVICE, &name).map_err(|e| format!("钥匙串不可用: {e}"))
}

impl KeychainBackend for SystemKeychain {
    fn read(&self, dir: &Path) -> Result<Option<HashMap<String, AccountSecret>>, String> {
        let e = entry_for(dir)?;
        match e.get_password() {
            Ok(raw) => serde_json::from_str(&raw)
                .map(Some)
                .map_err(|err| format!("解析钥匙串凭据失败: {err}")),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(format!("读取钥匙串失败: {err}")),
        }
    }

    fn write(&self, dir: &Path, map: &HashMap<String, AccountSecret>) -> Result<(), String> {
        let e = entry_for(dir)?;
        let raw = serde_json::to_string(map).map_err(|err| err.to_string())?;
        e.set_password(&raw)
            .map_err(|err| format!("写入钥匙串失败: {err}"))
    }
}

fn file_path(dir: &Path) -> PathBuf {
    dir.join("secrets.json")
}

fn write_marker(dir: &Path) -> Result<(), String> {
    let marker = serde_json::json!({ "backend": "keychain" });
    let s = serde_json::to_string_pretty(&marker).map_err(|e| e.to_string())?;
    crate::store::atomic_write_bytes(&file_path(dir), s.as_bytes())
}

fn write_file_map(dir: &Path, map: &HashMap<String, AccountSecret>) -> Result<(), String> {
    let s = serde_json::to_string_pretty(map).map_err(|e| e.to_string())?;
    crate::store::atomic_write_bytes(&file_path(dir), s.as_bytes())
}

fn backup_corrupt(p: &Path) -> Option<String> {
    let backup = p.with_extension(format!(
        "json.corrupt.{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    ));
    fs::rename(p, &backup).ok()?;
    backup.file_name().map(|n| n.to_string_lossy().into_owned())
}

fn read_file_state(dir: &Path) -> Result<FileState, String> {
    let p = file_path(dir);
    match fs::read_to_string(&p) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(FileState::Missing),
        Err(e) => Err(format!("读取 secrets.json 失败: {e}")),
        // 空文件不是合法状态(写入永远是完整 JSON),按截断损坏处理,不能静默当空表
        Ok(raw) if raw.trim().is_empty() => {
            let backup = backup_corrupt(&p);
            Err(format!(
                "secrets.json 为空,疑似写入被截断（备份: {}）",
                backup.as_deref().unwrap_or("失败")
            ))
        }
        Ok(raw) => match serde_json::from_str::<SecretsFile>(&raw) {
            Ok(SecretsFile::Map(m)) => Ok(FileState::Map(m)),
            Ok(SecretsFile::Marker(_)) => Ok(FileState::Marker),
            Err(e) => {
                let backup = backup_corrupt(&p);
                Err(format!(
                    "解析 secrets.json 失败（备份: {}）: {e}",
                    backup.as_deref().unwrap_or("失败")
                ))
            }
        },
    }
}

pub fn load(dir: &Path) -> Result<HashMap<String, AccountSecret>, String> {
    load_with(&SystemKeychain, dir)
}

pub(crate) fn load_with(
    kc: &dyn KeychainBackend,
    dir: &Path,
) -> Result<HashMap<String, AccountSecret>, String> {
    match read_file_state(dir)? {
        FileState::Marker => match kc.read(dir) {
            Ok(Some(m)) => Ok(m),
            Ok(None) => Err("凭据标记指向系统钥匙串,但钥匙串中没有对应条目".into()),
            Err(e) => Err(format!("凭据保存在系统钥匙串中，但当前无法读取：{e}")),
        },
        FileState::Map(m) => {
            // 文件为准(见模块头);非空时尝试迁回钥匙串,成功才用 marker 替掉明文
            if !m.is_empty() && kc.write(dir, &m).is_ok() {
                let _ = write_marker(dir);
            }
            Ok(m)
        }
        FileState::Missing => match kc.read(dir) {
            Ok(Some(m)) => {
                // marker 丢失但钥匙串有数据:补回 marker,下次不再依赖这个分支
                let _ = write_marker(dir);
                Ok(m)
            }
            // 无文件、无钥匙串条目(或钥匙串整体不可用) → 全新环境
            Ok(None) | Err(_) => Ok(HashMap::new()),
        },
    }
}

pub fn save(dir: &Path, map: &HashMap<String, AccountSecret>) -> Result<(), String> {
    save_with(&SystemKeychain, dir, map)
}

/// 在跨进程锁内更新单个账户，避免两个 CLI 各自读旧整表后互相覆盖新 token。
pub fn update_account(
    dir: &Path,
    account_id: &str,
    secret: Option<AccountSecret>,
) -> Result<HashMap<String, AccountSecret>, String> {
    update_account_with(&SystemKeychain, dir, account_id, secret)
}

fn update_account_with(
    kc: &dyn KeychainBackend,
    dir: &Path,
    account_id: &str,
    secret: Option<AccountSecret>,
) -> Result<HashMap<String, AccountSecret>, String> {
    use fs2::FileExt;

    fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
    let lock_path = dir.join("secrets.lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&lock_path)
        .map_err(|e| format!("打开凭据锁失败: {e}"))?;
    lock.lock_exclusive()
        .map_err(|e| format!("获取凭据锁失败: {e}"))?;

    let result = (|| {
        let mut latest = load_with(kc, dir)?;
        match secret {
            Some(value) => {
                latest.insert(account_id.to_string(), value);
            }
            None => {
                latest.remove(account_id);
            }
        }
        save_with(kc, dir, &latest)?;
        Ok(latest)
    })();
    let _ = lock.unlock();
    result
}

pub(crate) fn save_with(
    kc: &dyn KeychainBackend,
    dir: &Path,
    map: &HashMap<String, AccountSecret>,
) -> Result<(), String> {
    match kc.write(dir, map) {
        Ok(()) => write_marker(dir).or_else(|marker_err| {
            // 钥匙串已是新数据,但 marker 写失败会让磁盘留着旧明文;
            // 文件优先语义下旧明文会在下次 load 顶掉新数据 → 至少把文件同步成当前数据
            write_file_map(dir, map)
                .map_err(|e| format!("{marker_err}; 回退写 secrets.json 也失败: {e}"))
        }),
        // 钥匙串写失败 → 明文回退;map 文件本身就覆盖了 marker,权威性随之转移到文件
        Err(_kc_err) => write_file_map(dir, map),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static N: AtomicUsize = AtomicUsize::new(0);

    fn temp() -> PathBuf {
        let id = N.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!(
            "sealmail-sec-{}-{}-{}",
            std::process::id(),
            id,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|t| t.as_secs())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn secret(pw: &str) -> AccountSecret {
        AccountSecret {
            password: pw.into(),
            smtp_password: None,
            oauth: None,
        }
    }

    fn map_of(id: &str, pw: &str) -> HashMap<String, AccountSecret> {
        let mut m = HashMap::new();
        m.insert(id.into(), secret(pw));
        m
    }

    /// 假钥匙串:内容放内存,可注入读/写失败。
    #[derive(Default)]
    struct FakeKeychain {
        data: RefCell<Option<HashMap<String, AccountSecret>>>,
        fail_read: bool,
        fail_write: bool,
    }

    impl KeychainBackend for FakeKeychain {
        fn read(&self, _dir: &Path) -> Result<Option<HashMap<String, AccountSecret>>, String> {
            if self.fail_read {
                return Err("keychain locked".into());
            }
            Ok(self.data.borrow().clone())
        }
        fn write(&self, _dir: &Path, map: &HashMap<String, AccountSecret>) -> Result<(), String> {
            if self.fail_write {
                return Err("keychain locked".into());
            }
            *self.data.borrow_mut() = Some(map.clone());
            Ok(())
        }
    }

    #[derive(Default)]
    struct FileOnlyKeychain;

    impl KeychainBackend for FileOnlyKeychain {
        fn read(&self, _dir: &Path) -> Result<Option<HashMap<String, AccountSecret>>, String> {
            Err("keychain unavailable".into())
        }

        fn write(
            &self,
            _dir: &Path,
            _map: &HashMap<String, AccountSecret>,
        ) -> Result<(), String> {
            Err("keychain unavailable".into())
        }
    }

    #[test]
    fn file_roundtrip_when_used_as_fallback() {
        let dir = temp();
        write_file_map(&dir, &map_of("a1", "p")).unwrap();
        let kc = FakeKeychain {
            fail_read: true,
            fail_write: true,
            ..Default::default()
        };
        let loaded = load_with(&kc, &dir).unwrap();
        assert_eq!(loaded.get("a1").unwrap().password, "p");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn marker_file_is_not_treated_as_account_map() {
        let dir = temp();
        write_marker(&dir).unwrap();
        assert!(matches!(read_file_state(&dir), Ok(FileState::Marker)));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn truncated_empty_secrets_file_is_error_with_backup_not_empty_map() {
        let dir = temp();
        fs::write(file_path(&dir), "").unwrap();
        let kc = FakeKeychain::default();
        let err = load_with(&kc, &dir).expect_err("空文件必须报错,不能静默当无账户");
        assert!(err.contains("截断"), "错误应说明疑似截断: {err}");
        let backups: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("corrupt"))
            .collect();
        assert!(!backups.is_empty(), "应留下 .corrupt 备份");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn newer_file_fallback_wins_over_stale_keychain() {
        // 钥匙串故障期间 save 落到文件;故障恢复后 load 绝不能用旧钥匙串数据顶掉文件里的新凭据
        let dir = temp();
        let kc = FakeKeychain::default();
        *kc.data.borrow_mut() = Some(map_of("a1", "old-revoked-token"));
        write_file_map(&dir, &map_of("a1", "new-token")).unwrap();

        let loaded = load_with(&kc, &dir).unwrap();
        assert_eq!(loaded.get("a1").unwrap().password, "new-token");
        // 迁移完成:钥匙串已更新为新数据,磁盘明文被 marker 替换
        assert_eq!(
            kc.data.borrow().as_ref().unwrap().get("a1").unwrap().password,
            "new-token"
        );
        assert!(matches!(read_file_state(&dir), Ok(FileState::Marker)));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn marker_with_unreadable_keychain_is_hard_error() {
        let dir = temp();
        write_marker(&dir).unwrap();
        let kc = FakeKeychain {
            fail_read: true,
            ..Default::default()
        };
        let err = load_with(&kc, &dir).expect_err("marker 在而钥匙串读不出必须报错");
        assert!(err.contains("钥匙串"), "{err}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn marker_with_missing_keychain_entry_is_error_not_empty() {
        let dir = temp();
        write_marker(&dir).unwrap();
        let kc = FakeKeychain::default(); // 可用但无条目
        assert!(load_with(&kc, &dir).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn save_success_leaves_marker_only_no_plaintext() {
        let dir = temp();
        let kc = FakeKeychain::default();
        save_with(&kc, &dir, &map_of("a1", "pw")).unwrap();
        let raw = fs::read_to_string(file_path(&dir)).unwrap();
        assert!(!raw.contains("pw"), "钥匙串成功后磁盘不能有明文: {raw}");
        assert_eq!(load_with(&kc, &dir).unwrap().get("a1").unwrap().password, "pw");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn save_keychain_failure_falls_back_to_file_and_transfers_authority() {
        let dir = temp();
        // 先正常保存一版(钥匙串+marker)
        let kc_ok = FakeKeychain::default();
        save_with(&kc_ok, &dir, &map_of("a1", "old")).unwrap();
        // 钥匙串坏了,保存新凭据 → 落文件,marker 被覆盖
        let kc_broken = FakeKeychain {
            data: RefCell::new(kc_ok.data.borrow().clone()),
            fail_write: true,
            fail_read: false,
        };
        save_with(&kc_broken, &dir, &map_of("a1", "new")).unwrap();
        assert!(matches!(read_file_state(&dir), Ok(FileState::Map(_))));
        // 即使钥匙串里还躺着旧数据,load 也必须回新凭据
        assert_eq!(
            load_with(&kc_broken, &dir).unwrap().get("a1").unwrap().password,
            "new"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn mixed_marker_and_accounts_file_is_not_silently_dropped() {
        // 同时含 backend 键和账户数据的文件不能按 marker 丢弃账户
        let dir = temp();
        fs::write(
            file_path(&dir),
            r#"{"backend":"keychain","a1":{"password":"p"}}"#,
        )
        .unwrap();
        match read_file_state(&dir) {
            Ok(FileState::Marker) => panic!("混合文件被误判为 marker,账户数据会被丢弃"),
            _ => {}
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn concurrent_account_updates_preserve_both_changes() {
        use std::sync::{Arc, Barrier};

        let dir = temp();
        write_file_map(&dir, &map_of("base", "base-pw")).unwrap();
        let start = Arc::new(Barrier::new(3));
        let mut joins = Vec::new();
        for (id, pw) in [("a1", "pw1"), ("a2", "pw2")] {
            let dir = dir.clone();
            let start = start.clone();
            joins.push(std::thread::spawn(move || {
                start.wait();
                update_account_with(&FileOnlyKeychain, &dir, id, Some(secret(pw))).unwrap();
            }));
        }
        start.wait();
        for join in joins {
            join.join().unwrap();
        }

        let loaded = load_with(&FileOnlyKeychain, &dir).unwrap();
        assert_eq!(loaded.get("base").unwrap().password, "base-pw");
        assert_eq!(loaded.get("a1").unwrap().password, "pw1");
        assert_eq!(loaded.get("a2").unwrap().password, "pw2");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn account_update_waits_for_an_existing_process_lock() {
        use fs2::FileExt;
        use std::fs::OpenOptions;
        use std::sync::mpsc;
        use std::time::Duration;

        let dir = temp();
        write_file_map(&dir, &map_of("base", "base-pw")).unwrap();
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(dir.join("secrets.lock"))
            .unwrap();
        lock.lock_exclusive().unwrap();

        let (done_tx, done_rx) = mpsc::channel();
        let update_dir = dir.clone();
        let join = std::thread::spawn(move || {
            let result = update_account_with(
                &FileOnlyKeychain,
                &update_dir,
                "a1",
                Some(secret("pw1")),
            );
            done_tx.send(result).unwrap();
        });
        assert!(
            done_rx.recv_timeout(Duration::from_millis(100)).is_err(),
            "并发更新必须等待已有的跨进程锁"
        );

        lock.unlock().unwrap();
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("释放锁后更新应完成")
            .unwrap();
        join.join().unwrap();
        let _ = fs::remove_dir_all(dir);
    }
}
