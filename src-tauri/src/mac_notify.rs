//! macOS 系统通知：进程级单例 delegate + 「通知 identifier → 邮件目标」精确路由。
//!
//! 为什么不用现成方案：
//! - tauri-plugin-notification 在 macOS 用废弃的 NSUserNotification 且发完即走，
//!   不监听点击（actionPerformed 仅移动端），点通知毫无反应；
//! - mac-notification-sys 虽能 wait_for_click，但每次 send() 都会**替换**
//!   NSUserNotificationCenter 的全局 delegate，且回调不校验是哪条通知——
//!   邮件客户端常态是多条通知并存，点击任何一条都只会唤醒最后一条的等待线程：
//!   要么打开错误的邮件，要么毫无反应（v0.1.33 的缺陷根源）。
//!
//! 这里改为：进程内只安装一次自己的 delegate；每条通知带唯一 identifier；
//! didActivateNotification 回调在主线程按 identifier 查表精确路由。不阻塞任何线程，
//! 点「通知中心里攒下的旧通知」也能各自打开对应邮件（仅限本次进程发出的；
//! 重启后旧通知点击只唤起窗口）。
//!
//! NSUserNotification 虽已废弃，但与 tauri-plugin-notification 用的是同一套 API，
//! examples/notify_probe.rs 实测 macOS 26 上点击回调仍然可用。

#![cfg(target_os = "macos")]
#![allow(deprecated)] // NSUserNotification 全家族带 deprecated 标记

use crate::logging;
use crate::watcher::{self, NotificationMailTarget};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, MainThreadOnly};
use objc2_foundation::{
    MainThreadMarker, NSObject, NSObjectProtocol, NSString, NSUserNotification,
    NSUserNotificationCenter, NSUserNotificationCenterDelegate,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter};

/// identifier → 点击后要打开的邮件目标（None = 纯提示通知，点击只唤起窗口）
fn targets() -> &'static Mutex<HashMap<String, Option<NotificationMailTarget>>> {
    static T: OnceLock<Mutex<HashMap<String, Option<NotificationMailTarget>>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(HashMap::new()))
}

static APP: OnceLock<AppHandle> = OnceLock::new();
static SEQ: AtomicU64 = AtomicU64::new(0);

// delegate 只在主线程创建/使用（回调也在主线程），用 thread_local 持有防止被释放
// （NSUserNotificationCenter.delegate 是 assign 弱引用，不持有会悬垂）。
thread_local! {
    static DELEGATE: RefCell<Option<Retained<NotifyDelegate>>> = const { RefCell::new(None) };
}

define_class!(
    // SAFETY: NSObject 无子类化限制；不重写父类方法、不自持。
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SealMailNotifyDelegate"]
    struct NotifyDelegate;

    unsafe impl NSObjectProtocol for NotifyDelegate {}

    unsafe impl NSUserNotificationCenterDelegate for NotifyDelegate {
        // 用户点击横幅正文/动作按钮（含通知中心里的旧通知）→ 按 identifier 精确路由
        #[unsafe(method(userNotificationCenter:didActivateNotification:))]
        fn did_activate(
            &self,
            center: &NSUserNotificationCenter,
            notification: &NSUserNotification,
        ) {
            let id = notification
                .identifier()
                .map(|s| s.to_string())
                .unwrap_or_default();
            let target = targets().lock().unwrap().remove(&id).flatten();
            logging::log(format!("[notify] 点击回调 id={id} target={target:?}"));
            if let Some(app) = APP.get() {
                if let Some(t) = target {
                    watcher::set_pending_notification_target_now(t);
                }
                watcher::reveal_main_window(app);
                let _ = app.emit("notification-activated", ());
            }
            center.removeDeliveredNotification(notification);
        }
    }
);

impl NotifyDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm);
        unsafe { msg_send![this, init] }
    }
}

/// 主线程：确保 delegate 已安装（幂等，且每次发通知前重申一次，防止被第三方替换）。
fn ensure_delegate_installed(mtm: MainThreadMarker) {
    DELEGATE.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(NotifyDelegate::new(mtm));
            logging::log("[notify] delegate 已创建");
        }
        let center = NSUserNotificationCenter::defaultUserNotificationCenter();
        let delegate = slot.as_ref().unwrap();
        // SAFETY: delegate 由 thread_local 持有，与进程同生命周期
        unsafe { center.setDelegate(Some(ProtocolObject::from_ref(&**delegate))) };
    });
}

/// dev（未打包）运行时二进制没有 bundle id，NSUserNotificationCenter 发不出通知；
/// 借 mac-notification-sys 的 set_application 把 mainBundle 伪装成已安装的 SealMail。
/// 打包后的 .app 自带真实 bundle id，跳过（不在生产进程里做 swizzle）。
fn ensure_dev_identity(app: &AppHandle) {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let bundled = std::env::current_exe()
            .map(|p| p.to_string_lossy().contains(".app/Contents/MacOS"))
            .unwrap_or(false);
        if bundled {
            return;
        }
        let id = app.config().identifier.clone();
        match mac_notification_sys::set_application(&id) {
            Ok(_) => logging::log(format!("[notify] dev 身份伪装为 {id}")),
            Err(e) => logging::log(format!("[notify] dev set_application({id}) 失败: {e:?}")),
        }
    });
}

/// 发送一条通知（任意线程可调用）。target 为点击后要打开的邮件。
pub fn notify(app: &AppHandle, title: String, body: String, target: Option<NotificationMailTarget>) {
    let _ = APP.set(app.clone());
    ensure_dev_identity(app);
    let id = format!(
        "sealmail-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    );
    targets().lock().unwrap().insert(id.clone(), target);
    logging::log(format!("[notify] 投递通知 id={id} title={title:?}"));
    let result = app.run_on_main_thread(move || {
        let Some(mtm) = MainThreadMarker::new() else {
            logging::log("[notify] run_on_main_thread 未在主线程执行？");
            return;
        };
        ensure_delegate_installed(mtm);
        let n = NSUserNotification::new();
        n.setIdentifier(Some(&NSString::from_str(&id)));
        n.setTitle(Some(&NSString::from_str(&title)));
        n.setInformativeText(Some(&NSString::from_str(&body)));
        let center = NSUserNotificationCenter::defaultUserNotificationCenter();
        center.deliverNotification(&n);
    });
    if let Err(e) = result {
        logging::log(format!("[notify] 通知投递失败（事件循环不可用）: {e}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_target(uid: u32) -> NotificationMailTarget {
        NotificationMailTarget {
            account_id: "acc-1".to_string(),
            folder: "INBOX".to_string(),
            uid: Some(uid),
            message_id: None,
        }
    }

    // 锁定 identifier 路由的核心不变量：各条通知的目标互不串线、
    // 取走即消费（同一条不会二次触发）、未知 id 不给目标（只唤窗口）。
    // ObjC delegate 层无法在无头环境单测，此表即路由正确性的测试缝隙。
    #[test]
    fn identifier_routing_is_exact_and_consume_once() {
        let map = targets();
        map.lock().unwrap().clear();

        map.lock().unwrap().insert("id-a".into(), Some(sample_target(1)));
        map.lock().unwrap().insert("id-b".into(), Some(sample_target(2)));

        // 点 A 只拿到 A 的目标，B 不受影响
        let a = map.lock().unwrap().remove("id-a").flatten();
        assert_eq!(a.unwrap().uid, Some(1));
        assert!(map.lock().unwrap().contains_key("id-b"), "B 的目标不应被消费");

        // 同一条第二次点击拿不到目标（已消费）
        assert!(map.lock().unwrap().remove("id-a").flatten().is_none());

        // 未知 id（如重启前的旧通知）没有目标
        assert!(map.lock().unwrap().remove("id-stale").flatten().is_none());

        map.lock().unwrap().clear();
    }
}
