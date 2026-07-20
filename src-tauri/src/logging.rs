//! 轻量文件日志：通知链路与关键命令耗时写入 `<app配置目录>/logs/sealmail.log`。
//! 真机上出问题时用户可直接把日志文件发回来定位（菜单里没有入口，路径见 HANDOFF）。
//! 单文件上限 1MB，超限时轮转为 sealmail.log.old（保留上一份）。
//! 只在事件级别写（通知、邮件切换等用户动作），不进热循环，对性能无感。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static WRITE_LOCK: Mutex<()> = Mutex::new(());
const MAX_LOG_BYTES: u64 = 1024 * 1024;

/// 在拿到应用配置目录后尽早调用一次（lib.rs setup）。
pub fn init(config_dir: &std::path::Path) {
    let dir = config_dir.join("logs");
    let _ = fs::create_dir_all(&dir);
    let _ = LOG_PATH.set(dir.join("sealmail.log"));
    log(format!(
        "——— SealMail v{} 启动 ———",
        env!("CARGO_PKG_VERSION")
    ));
}

/// 追加一行日志（带本地时间戳）。init 前调用只打到 stderr。
pub fn log(msg: impl AsRef<str>) {
    let line = format!(
        "[{}] {}\n",
        chrono::Local::now().format("%m-%d %H:%M:%S%.3f"),
        msg.as_ref()
    );
    eprint!("{line}");
    let Some(path) = LOG_PATH.get() else { return };
    // 若曾有线程持锁 panic，恢复而不是永久毒化（日志绝不能反过来把业务搞挂）
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Ok(meta) = fs::metadata(path) {
        if meta.len() > MAX_LOG_BYTES {
            let _ = fs::rename(path, path.with_extension("log.old"));
        }
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}
