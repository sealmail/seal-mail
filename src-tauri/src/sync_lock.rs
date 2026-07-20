//! 跨进程目录同步互斥：GUI 可同时 fork 多个 CLI，对同一 (account, folder) 串行化，
//! 避免两路删除检测互相误伤对方刚插入的新邮件。
//!
//! 用 OS 咨询锁(flock/LockFileEx)而不是"文件存在性 + mtime 过期"：
//! - 进程崩溃时内核自动释放,没有残留锁,也就不需要 stale 抢占(旧方案里
//!   抢占是 check→remove→create 的 TOCTOU,两个等待者会互删对方的新锁;
//!   长于过期窗口的合法同步还会被误抢)。
//! - 锁文件永不删除:unlink 一个别人正持有 flock 的文件后,新打开者拿到的是
//!   新 inode 上的锁,两把"互斥锁"就互相看不见了。

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct FolderSyncLock {
    // 持有 fd 即持有锁;Drop 关闭 fd 自动解锁。文件本身留在磁盘上。
    _file: File,
}

fn lock_path(config_dir: &Path, account_id: &str, folder: &str) -> PathBuf {
    use sha2::{Digest, Sha256};
    let locks = config_dir.join("locks");
    let sanitize = |s: &str| -> String {
        s.chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect()
    };
    // 归一化会让不同目录名撞车(非 ASCII 全变 '_'),补一段原始名的摘要保证一一对应
    let digest = Sha256::digest(format!("{account_id}\u{0}{folder}").as_bytes());
    locks.join(format!(
        "{}__{}-{}.lock",
        sanitize(account_id),
        sanitize(folder),
        hex::encode(&digest[..6])
    ))
}

/// 尝试获取目录同步锁。已被其他进程/线程占用时返回可读错误(不等待)。
pub fn try_acquire(
    config_dir: &Path,
    account_id: &str,
    folder: &str,
) -> Result<FolderSyncLock, String> {
    use fs2::FileExt;
    let path = lock_path(config_dir, account_id, folder);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建同步锁目录失败: {e}"))?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&path)
        .map_err(|e| format!("打开同步锁文件失败: {e}"))?;
    match file.try_lock_exclusive() {
        Ok(()) => {
            // 内容仅供人工排查,锁语义完全由 flock 承担
            let _ = file.set_len(0);
            let mut f = &file;
            let _ = writeln!(f, "pid={} ts={}", std::process::id(), now_unix());
            Ok(FolderSyncLock { _file: file })
        }
        Err(ref e) if e.kind() == fs2::lock_contended_error().kind() => {
            Err(format!("目录 {folder} 正在同步中，请稍后再试"))
        }
        Err(e) => Err(format!("获取同步锁失败: {e}")),
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
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

    #[test]
    fn leftover_lock_file_from_crash_does_not_block() {
        // 崩溃残留的锁文件(哪怕 mtime 很新)不应让后续同步等 15 分钟:
        // 锁的持有状态跟进程/fd 生命周期绑定,文件存在与否不代表有人持锁
        let dir = temp_dir();
        let path = lock_path(&dir, "acc1", "INBOX");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "pid=99999 ts=0").unwrap();
        try_acquire(&dir, "acc1", "INBOX").expect("残留锁文件不应阻塞新的同步");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn distinct_non_ascii_folders_get_distinct_locks() {
        // 归一化把非 ASCII 全变 '_':两个不同中文目录不能共享一把锁
        let dir = temp_dir();
        let a = try_acquire(&dir, "acc1", "已发送").unwrap();
        let b = try_acquire(&dir, "acc1", "垃圾邮件").expect("不同目录不应互斥");
        drop(a);
        drop(b);
        let _ = fs::remove_dir_all(dir);
    }
}
