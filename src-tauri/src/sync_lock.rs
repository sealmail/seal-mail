//! 跨进程目录同步互斥：GUI 可同时 fork 多个 CLI，对同一 (account, folder) 串行化，
//! 避免两路删除检测互相误伤对方刚插入的新邮件。

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// 崩溃残留锁超过此时长视为过期，可抢占
const STALE_AFTER: Duration = Duration::from_secs(15 * 60);

pub struct FolderSyncLock {
    path: PathBuf,
    _file: File,
}

impl Drop for FolderSyncLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn lock_path(config_dir: &Path, account_id: &str, folder: &str) -> PathBuf {
    let locks = config_dir.join("locks");
    let safe_account: String = account_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let safe_folder: String = folder
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    locks.join(format!("{safe_account}__{safe_folder}.lock"))
}

fn is_stale(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return true;
    };
    let Ok(modified) = meta.modified() else {
        return true;
    };
    SystemTime::now()
        .duration_since(modified)
        .map(|d| d > STALE_AFTER)
        .unwrap_or(true)
}

/// 尝试获取目录同步锁。已被占用且未过期时返回可读错误。
pub fn try_acquire(config_dir: &Path, account_id: &str, folder: &str) -> Result<FolderSyncLock, String> {
    let path = lock_path(config_dir, account_id, folder);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建同步锁目录失败: {e}"))?;
    }
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut f) => {
            let _ = writeln!(f, "pid={} ts={}", std::process::id(), now_unix());
            let _ = f.sync_all();
            Ok(FolderSyncLock { path, _file: f })
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            if is_stale(&path) {
                let _ = fs::remove_file(&path);
                return try_acquire(config_dir, account_id, folder);
            }
            Err(format!(
                "目录 {folder} 正在同步中，请稍后再试"
            ))
        }
        Err(e) => Err(format!("获取同步锁失败: {e}")),
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static N: AtomicUsize = AtomicUsize::new(0);

    fn temp_dir() -> PathBuf {
        let id = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "sealmail-sync-lock-{}-{}-{}",
            std::process::id(),
            id,
            now_unix()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn second_acquire_fails_while_first_held() {
        let dir = temp_dir();
        let a = try_acquire(&dir, "acc1", "INBOX").expect("first lock");
        let err = match try_acquire(&dir, "acc1", "INBOX") {
            Ok(_) => panic!("second lock must fail while first is held"),
            Err(e) => e,
        };
        assert!(err.contains("正在同步") || err.contains("sync"), "{err}");
        drop(a);
        let b = try_acquire(&dir, "acc1", "INBOX").expect("after drop");
        drop(b);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn different_folders_do_not_block() {
        let dir = temp_dir();
        let a = try_acquire(&dir, "acc1", "INBOX").unwrap();
        let b = try_acquire(&dir, "acc1", "Sent").unwrap();
        drop(a);
        drop(b);
        let _ = fs::remove_dir_all(dir);
    }
}
