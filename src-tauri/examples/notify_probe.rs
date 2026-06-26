//! 点击探针：验证“自己用 mac-notification-sys、wait_for_click(true) 发通知后，
//! 用户点击横幅能否被捕获”。这是根治方案的基石假设——在废弃的 NSUserNotification 上，
//! 现代 macOS 是否还会回调点击。
//!
//! 运行：
//!   cargo run --manifest-path src-tauri/Cargo.toml --example notify_probe
//! 然后【点击弹出的通知横幅本身】（点正文，不要划走/不要点关闭），看终端打印。
//!   RESULT => Click            ← 点击能被捕获 → 根治方案可行
//!   RESULT => None             ← 没捕获到（系统不再回调废弃 API）→ 需改用现代 API
//!   RESULT => CloseButton(..)  ← 你点到了关闭按钮，请重试并点横幅正文

#[cfg(target_os = "macos")]
fn main() {
    use mac_notification_sys::{set_application, Notification};

    // 让通知以已安装的 SealMail 身份出现；也顺带验证 bundle 能否被系统解析到。
    match set_application("com.sealmail.app") {
        Ok(_) => println!("[probe] set_application(com.sealmail.app) OK"),
        Err(e) => println!("[probe] set_application 失败（继续用默认身份）: {e:?}"),
    }

    println!("[probe] 即将发送一条通知。请在它弹出后【点击横幅正文】……");

    let mut n = Notification::new();
    n.title("SealMail 点击探针")
        .message("请点击这条通知（点横幅正文，不要划走，不要点关闭）")
        .wait_for_click(true);

    match n.send() {
        Ok(resp) => println!("[probe] RESULT => {resp:?}"),
        Err(e) => println!("[probe] ERROR => {e:?}"),
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("[probe] 此探针仅用于 macOS");
}
