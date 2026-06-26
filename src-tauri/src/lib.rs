pub mod cli;
pub mod core;
pub mod crypto;
pub mod db;
pub mod filters;
pub mod imap_client;
pub mod ledger;
pub mod mail;
pub mod models;
pub mod oauth;
pub mod pop3_client;
pub mod smtp_client;
pub mod store;
pub mod updater;
pub mod watcher;

use crate::core::AppStateView;
use models::*;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use serde::Serialize;
use store::{AppState, StoreData};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

fn gen_id() -> String {
    let mut b = [0u8; 8];
    let _ = getrandom::getrandom(&mut b);
    hex::encode(b)
}

fn cli_binary_name() -> &'static str {
    if cfg!(windows) {
        "sealmail-cli.exe"
    } else {
        "sealmail-cli"
    }
}

fn cli_candidates(app: &AppHandle) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(path) = std::env::var("SEALMAIL_CLI_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            candidates.push(PathBuf::from(trimmed));
        }
    }
    if let Ok(current) = std::env::current_exe() {
        if let Some(dir) = current.parent() {
            candidates.push(dir.join(cli_binary_name()));
            candidates.push(dir.join("bin").join(cli_binary_name()));
        }
    }
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join(cli_binary_name()));
        candidates.push(resource_dir.join("bin").join(cli_binary_name()));
    }
    candidates
}

fn resolve_cli(app: &AppHandle) -> Result<(PathBuf, bool), String> {
    let candidates = cli_candidates(app);
    if let Some(path) = candidates.iter().find(|path| path.is_file()).cloned() {
        return Ok((path, false));
    }
    let current = std::env::current_exe().map_err(|e| format!("无法定位当前 App 可执行文件: {e}"))?;
    if current.is_file() {
        return Ok((current, true));
    }
    let searched = candidates
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!("找不到 sealmail-cli，也无法回退到 App CLI 模式。已查找: {searched}"))
}

#[tauri::command]
fn cli_json(
    app: AppHandle,
    args: Vec<String>,
    stdin: Option<String>,
    env: Option<HashMap<String, String>>,
) -> Result<serde_json::Value, String> {
    if args.is_empty() {
        return Err("CLI 参数不能为空".into());
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Err("GUI bridge 只允许 JSON 命令，不执行 help".into());
    }
    let (cli, app_cli_mode) = resolve_cli(&app)?;
    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let mut final_args = args;
    if !final_args.iter().any(|arg| arg == "--json") {
        final_args.push("--json".into());
    }
    let mut command = Command::new(cli);
    command
        .args(&final_args)
        .env("SEALMAIL_CONFIG_DIR", config_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if app_cli_mode {
        command.env("SEALMAIL_RUN_CLI", "1");
    }
    if let Some(env) = env {
        for (key, value) in env {
            if key.trim().is_empty() || key.contains('=') || key.contains('\0') {
                return Err("CLI env key 无效".into());
            }
            command.env(key, value);
        }
    }
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().map_err(|e| format!("启动 CLI 失败: {e}"))?;
    if let Some(input) = stdin {
        let Some(mut child_stdin) = child.stdin.take() else {
            return Err("打开 CLI stdin 失败".into());
        };
        child_stdin
            .write_all(input.as_bytes())
            .map_err(|e| format!("写入 CLI stdin 失败: {e}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("等待 CLI 失败: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            return Err(format!("CLI 执行失败: {}", output.status));
        }
        return Err(stderr);
    }
    serde_json::from_slice(&output.stdout).map_err(|e| {
        let stdout = String::from_utf8_lossy(&output.stdout);
        format!("CLI 输出不是 JSON: {e}; stdout={stdout}")
    })
}

#[tauri::command]
fn open_external_url(app: AppHandle, url: String) -> Result<(), String> {
    let trimmed = url.trim();
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err("只允许打开 http/https 外部链接".into());
    }
    app.opener()
        .open_url(trimmed.to_string(), None::<String>)
        .map_err(|e| e.to_string())
}

// ───────────────────────── state / accounts ─────────────────────────

#[tauri::command]
fn get_state(state: State<'_, AppState>) -> Result<AppStateView, String> {
    let s = state.inner.lock().unwrap();
    Ok(core::state_view(&s))
}

// ───────────────────────── identity / ledger ─────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LedgerAccountRow {
    index: u32,
    path: String,
    address: String,
}

/// 一次设备会话取前 `count` 个 Ledger Live 地址（绑定时选择）。
#[tauri::command]
async fn ledger_get_addresses(count: Option<u32>) -> Result<Vec<LedgerAccountRow>, String> {
    let n = count.unwrap_or(5).min(10);
    tauri::async_runtime::spawn_blocking(move || {
        let indices: Vec<u32> = (0..n).collect();
        ledger::get_addresses(&indices).map(|rows| {
            rows.into_iter()
                .map(|(index, path, address)| LedgerAccountRow {
                    index,
                    path,
                    address,
                })
                .collect()
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn bind_ledger(
    state: State<'_, AppState>,
    path: String,
    address: String,
) -> Result<IdentityInfo, String> {
    let mut s = state.inner.lock().unwrap();
    core::bind_ledger(&mut s, path, address)
}

#[tauri::command]
fn use_local_key(state: State<'_, AppState>) -> Result<IdentityInfo, String> {
    let mut s = state.inner.lock().unwrap();
    core::use_local_key(&mut s)
}

// ───────────────────────── prefs ─────────────────────────

#[tauri::command]
fn get_close_behavior(state: State<'_, AppState>) -> String {
    let s = state.inner.lock().unwrap();
    core::get_close_behavior(&s)
}

#[tauri::command]
fn set_close_behavior(state: State<'_, AppState>, behavior: String) -> Result<String, String> {
    let mut s = state.inner.lock().unwrap();
    core::set_close_behavior(&mut s, behavior)
}

#[tauri::command]
fn get_notify_new_mail(state: State<'_, AppState>) -> bool {
    let s = state.inner.lock().unwrap();
    core::get_notify_new_mail(&s)
}

#[tauri::command]
fn set_notify_new_mail(state: State<'_, AppState>, enabled: bool) -> Result<bool, String> {
    let mut s = state.inner.lock().unwrap();
    core::set_notify_new_mail(&mut s, enabled)
}

/// 前端主动拉取待打开的通知邮件：顺手唤起主窗口，并返回（同时消费）待打开目标。
/// 返回 None 表示当前没有待打开通知。请求/响应方式比一次性 emit 更可靠——
/// 不会因为事件在监听器未就绪时被丢弃而把目标白白消费掉。
#[tauri::command]
fn open_pending_notification_mail(app: AppHandle) -> Option<watcher::NotificationMailTarget> {
    let target = watcher::take_pending_notification_target();
    if target.is_some() {
        watcher::reveal_main_window(&app);
    }
    target
}

// ───────────────────────── oauth2 (设备码) ─────────────────────────

#[tauri::command]
async fn oauth_begin_device(
    provider: String,
    client_id: Option<String>,
) -> Result<oauth::DeviceFlowStart, String> {
    let provider = oauth::OAuthProvider::parse(&provider)?;
    let cid = client_id
        .filter(|c| !c.trim().is_empty())
        .unwrap_or_else(|| match provider {
            oauth::OAuthProvider::Microsoft => oauth::DEFAULT_MS_CLIENT_ID.to_string(),
            oauth::OAuthProvider::Google => String::new(),
        });
    oauth::begin_device_flow(provider, cid.trim()).await
}

#[tauri::command]
async fn oauth_poll_device(
    provider: String,
    client_id: String,
    client_secret: Option<String>,
    device_code: String,
) -> Result<oauth::DevicePoll, String> {
    let provider = oauth::OAuthProvider::parse(&provider)?;
    oauth::poll_device_for(provider, &client_id, client_secret.as_deref(), &device_code).await
}

#[tauri::command]
fn oauth_begin_browser(
    provider: String,
    client_id: String,
    client_secret: Option<String>,
    login_hint: Option<String>,
) -> Result<oauth::BrowserFlowStart, String> {
    let provider = oauth::OAuthProvider::parse(&provider)?;
    oauth::begin_browser_flow(
        provider,
        &client_id,
        client_secret.as_deref(),
        login_hint.as_deref(),
    )
}

#[tauri::command]
async fn oauth_finish_browser(flow_id: String) -> Result<oauth::OAuthTokens, String> {
    oauth::finish_browser_flow(&flow_id).await
}

/// 取账户凭据；OAuth2 账户的 access_token 临近过期时先刷新并回写 secrets.json
async fn fresh_secret(
    state: &State<'_, AppState>,
    account_id: &str,
) -> Result<AccountSecret, String> {
    let secret = {
        let s = state.inner.lock().unwrap();
        s.secret(account_id)?
    };
    let Some(tokens) = &secret.oauth else {
        return Ok(secret);
    };
    if !tokens.needs_refresh() {
        return Ok(secret);
    }
    let refreshed = oauth::refresh_tokens(tokens).await?;
    let mut s = state.inner.lock().unwrap();
    let entry = s
        .secrets
        .get_mut(account_id)
        .ok_or_else(|| format!("账户密码缺失: {}", account_id))?;
    entry.oauth = Some(refreshed);
    let updated = entry.clone();
    s.save_secrets()?;
    Ok(updated)
}

#[tauri::command]
async fn test_connection(account: Account, secret: AccountSecret) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || core::test_connection(&account, &secret))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn add_account(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    mut account: Account,
    secret: AccountSecret,
) -> Result<Account, String> {
    if account.id.is_empty() {
        account.id = gen_id();
    }
    let (acc2, sec2) = (account.clone(), secret.clone());
    tauri::async_runtime::spawn_blocking(move || core::test_connection(&acc2, &sec2))
        .await
        .map_err(|e| e.to_string())??;
    let saved = {
        let mut s = state.inner.lock().unwrap();
        core::save_account(&mut s, account, secret)?
    };
    watcher::ensure_watchers(&app);
    Ok(saved)
}

#[tauri::command]
fn remove_account(state: State<'_, AppState>, account_id: String) -> Result<(), String> {
    let mut s = state.inner.lock().unwrap();
    core::remove_account(&mut s, account_id)
}

// ───────────────────────── folders ─────────────────────────

#[tauri::command]
async fn list_folders(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Vec<FolderInfo>, String> {
    fresh_secret(&state, &account_id).await?;
    let s = state.inner.lock().unwrap();
    core::list_folders(&s, &account_id)
}

#[tauri::command]
async fn create_folder(
    state: State<'_, AppState>,
    account_id: String,
    name: String,
) -> Result<(), String> {
    fresh_secret(&state, &account_id).await?;
    let mut s = state.inner.lock().unwrap();
    core::create_folder(&mut s, &account_id, name)
}

#[tauri::command]
async fn delete_folder(
    state: State<'_, AppState>,
    account_id: String,
    name: String,
) -> Result<(), String> {
    fresh_secret(&state, &account_id).await?;
    let mut s = state.inner.lock().unwrap();
    core::delete_folder(&mut s, &account_id, &name)
}

// ───────────────────────── messages ─────────────────────────

/// 从本地缓存读列表（秒出、可离线）。读取时重新解析+验证，信任列表变化即时生效。
#[tauri::command]
fn list_cached(
    state: State<'_, AppState>,
    account_id: String,
    folder: String,
    offset: u32,
    limit: u32,
) -> Result<core::CachedList, String> {
    let mut s = state.inner.lock().unwrap();
    core::list_cached(&mut s, &account_id, &folder, offset, limit)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncResult {
    added: u32,
    total: i64,
}

/// 与服务器增量同步：只下载新邮件；回扫最近窗口的已读/星标并检测服务器侧删除
#[tauri::command]
async fn sync_messages(
    state: State<'_, AppState>,
    account_id: String,
    folder: String,
) -> Result<SyncResult, String> {
    let secret = fresh_secret(&state, &account_id).await?;
    let mut s = state.inner.lock().unwrap();
    let result = core::sync_messages(&mut s, &account_id, &folder, &secret)?;
    Ok(SyncResult {
        added: result.added,
        total: result.total,
    })
}

/// 按需回填更早邮件：用户滚动/点击加载更多时调用。
#[tauri::command]
async fn sync_older_messages(
    state: State<'_, AppState>,
    account_id: String,
    folder: String,
) -> Result<SyncResult, String> {
    let secret = fresh_secret(&state, &account_id).await?;
    let mut s = state.inner.lock().unwrap();
    let result = core::sync_older_messages(&mut s, &account_id, &folder, &secret)?;
    Ok(SyncResult {
        added: result.added,
        total: result.total,
    })
}

#[tauri::command]
fn get_message(
    state: State<'_, AppState>,
    account_id: String,
    folder: String,
    uid: u32,
) -> Result<EmailFull, String> {
    let mut s = state.inner.lock().unwrap();
    core::get_message(&mut s, &account_id, &folder, uid)
}

/// Return all locally cached messages in the same folder conversation.
#[tauri::command]
fn list_thread(
    state: State<'_, AppState>,
    account_id: String,
    folder: String,
    thread_id: String,
) -> Result<Vec<EmailMeta>, String> {
    let mut s = state.inner.lock().unwrap();
    core::list_thread(&mut s, &account_id, &folder, &thread_id)
}

#[tauri::command]
async fn move_message(
    state: State<'_, AppState>,
    account_id: String,
    folder: String,
    uid: u32,
    target: String,
) -> Result<(), String> {
    let secret = fresh_secret(&state, &account_id).await?;
    let mut s = state.inner.lock().unwrap();
    core::move_message(&mut s, &account_id, &folder, uid, &target, &secret)
}

/// 一键归档：IMAP 移入服务器归档目录；POP3 移入本地「归档」虚拟目录。
#[tauri::command]
async fn archive_message(
    state: State<'_, AppState>,
    account_id: String,
    folder: String,
    uid: u32,
) -> Result<(), String> {
    let secret = fresh_secret(&state, &account_id).await?;
    let mut s = state.inner.lock().unwrap();
    core::archive_message(&mut s, &account_id, &folder, uid, &secret)
}

#[tauri::command]
async fn set_read(
    state: State<'_, AppState>,
    account_id: String,
    folder: String,
    uid: u32,
    read: bool,
) -> Result<(), String> {
    let secret = fresh_secret(&state, &account_id).await?;
    let mut s = state.inner.lock().unwrap();
    core::set_read(&mut s, &account_id, &folder, &[uid], read, &secret)
}

/// 批量标记已读/未读（「全部已读」一条连接完成）
#[tauri::command]
async fn mark_read(
    state: State<'_, AppState>,
    account_id: String,
    folder: String,
    uids: Vec<u32>,
    read: bool,
) -> Result<(), String> {
    let secret = fresh_secret(&state, &account_id).await?;
    let mut s = state.inner.lock().unwrap();
    core::set_read(&mut s, &account_id, &folder, &uids, read, &secret)
}

/// 星标/取消星标（IMAP \Flagged；POP3 仅本地记录）
#[tauri::command]
async fn set_flagged(
    state: State<'_, AppState>,
    account_id: String,
    folder: String,
    uid: u32,
    flagged: bool,
) -> Result<(), String> {
    let secret = fresh_secret(&state, &account_id).await?;
    let mut s = state.inner.lock().unwrap();
    core::set_flagged(&mut s, &account_id, &folder, uid, flagged, &secret)
}

/// permanent=false（默认）：移入回收站，可恢复；permanent=true：物理删除（前端仅在回收站内确认后使用）
#[tauri::command]
async fn delete_message(
    state: State<'_, AppState>,
    account_id: String,
    folder: String,
    uid: u32,
    permanent: Option<bool>,
) -> Result<(), String> {
    let permanent = permanent.unwrap_or(false);
    let secret = fresh_secret(&state, &account_id).await?;
    let mut s = state.inner.lock().unwrap();
    core::delete_message(&mut s, &account_id, &folder, uid, permanent, &secret)
}

/// 下载附件：优先用本地缓存的原文；缓存缺失时回源服务器
#[tauri::command]
async fn save_attachment(
    state: State<'_, AppState>,
    account_id: String,
    folder: String,
    uid: u32,
    index: usize,
    path: String,
) -> Result<(), String> {
    let cached = {
        let s = state.inner.lock().unwrap();
        db::get_raw(&s.db, &account_id, &folder, uid)?.is_some()
    };
    let secret = if cached {
        None
    } else {
        Some(fresh_secret(&state, &account_id).await?)
    };
    let s = state.inner.lock().unwrap();
    core::save_attachment(&s, &account_id, &folder, uid, index, &path, secret.as_ref()).map(|_| ())
}

// ───────────────────────── send ─────────────────────────

#[tauri::command]
async fn send_mail(
    state: State<'_, AppState>,
    account_id: String,
    to: Vec<String>,
    cc: Vec<String>,
    subject: String,
    body: String,
    sign: bool,
    attachments: Option<Vec<String>>,
) -> Result<smtp_client::SendResult, String> {
    // 在主线程先读附件，路径无效时尽早报错
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for path in attachments.unwrap_or_default() {
        let name = std::path::Path::new(&path)
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("附件路径无效: {}", path))?
            .to_string();
        let data = std::fs::read(&path).map_err(|e| format!("读取附件 {} 失败: {}", name, e))?;
        files.push((name, data));
    }
    let secret = fresh_secret(&state, &account_id).await?;
    let mut s = state.inner.lock().unwrap();
    core::send_mail(
        &mut s,
        &account_id,
        &secret,
        to,
        cc,
        &subject,
        &body,
        sign,
        files,
    )
}

/// 联系人查询（写信自动补全）：按往来次数+最近往来排序，最多 8 条
#[tauri::command]
fn list_contacts(state: State<'_, AppState>, query: Option<String>) -> Vec<Contact> {
    let s = state.inner.lock().unwrap();
    core::list_contacts(&s, query)
}

// ───────────────────────── drafts ─────────────────────────

#[tauri::command]
fn list_drafts(state: State<'_, AppState>) -> Vec<Draft> {
    let s = state.inner.lock().unwrap();
    core::list_drafts(&s)
}

#[tauri::command]
fn save_draft(state: State<'_, AppState>, draft: Draft) -> Result<Draft, String> {
    let mut s = state.inner.lock().unwrap();
    core::save_draft(&mut s, draft)
}

#[tauri::command]
fn delete_draft(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut s = state.inner.lock().unwrap();
    core::delete_draft(&mut s, id)
}

// ───────────────────────── filters ─────────────────────────

#[tauri::command]
fn save_filter(state: State<'_, AppState>, rule: FilterRule) -> Result<Vec<FilterRule>, String> {
    let mut s = state.inner.lock().unwrap();
    core::save_filter(&mut s, rule)
}

#[tauri::command]
fn delete_filter(state: State<'_, AppState>, id: String) -> Result<Vec<FilterRule>, String> {
    let mut s = state.inner.lock().unwrap();
    core::delete_filter(&mut s, id)
}

/// 对收件箱执行所有过滤规则：匹配则移动到目标目录
#[tauri::command]
async fn apply_filters(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<core::ApplyResult, String> {
    let secret = fresh_secret(&state, &account_id).await?;
    let mut s = state.inner.lock().unwrap();
    core::apply_filters(&mut s, &account_id, &secret)
}

// ───────────────────────── trust ─────────────────────────

#[tauri::command]
fn trust_sender(
    state: State<'_, AppState>,
    name: String,
    email: String,
    fingerprint: String,
    org: Option<String>,
) -> Result<Vec<TrustedContact>, String> {
    let mut s = state.inner.lock().unwrap();
    core::trust_sender(&mut s, name, email, fingerprint, org)
}

#[tauri::command]
fn remove_trusted(
    state: State<'_, AppState>,
    email: String,
) -> Result<Vec<TrustedContact>, String> {
    let mut s = state.inner.lock().unwrap();
    core::remove_trusted(&mut s, email)
}

// ───────────────────────── update ─────────────────────────

/// 签名自动升级不可用时的回退：直接查 GitHub Releases，引导手动下载
#[tauri::command]
async fn check_for_update() -> Result<updater::UpdateInfo, String> {
    updater::check_for_update().await
}

// ───────────────────────── entry ─────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init());
    #[cfg(desktop)]
    {
        builder = builder
            .plugin(tauri_plugin_updater::Builder::new().build())
            .plugin(tauri_plugin_process::init());
    }
    builder
        // 点关闭按钮：close_behavior = "hide" 时只隐藏窗口不退出（macOS 默认）
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let hide = window
                    .app_handle()
                    .try_state::<AppState>()
                    .map(|st| st.inner.lock().unwrap().prefs.close_behavior == "hide")
                    .unwrap_or(false);
                if hide {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            let dir = app.path().app_config_dir().expect("无法获取应用配置目录");
            let data = StoreData::load(dir).expect("初始化本地存储失败");
            app.manage(AppState {
                inner: std::sync::Mutex::new(data),
            });
            // 启动新邮件监听（IMAP IDLE / POP3 轮询）
            watcher::ensure_watchers(&app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            cli_json,
            get_state,
            open_external_url,
            check_for_update,
            oauth_begin_device,
            oauth_poll_device,
            oauth_begin_browser,
            oauth_finish_browser,
            ledger_get_addresses,
            bind_ledger,
            use_local_key,
            test_connection,
            add_account,
            remove_account,
            list_folders,
            create_folder,
            delete_folder,
            list_cached,
            sync_messages,
            sync_older_messages,
            get_message,
            list_thread,
            move_message,
            archive_message,
            set_read,
            mark_read,
            set_flagged,
            delete_message,
            send_mail,
            save_attachment,
            list_contacts,
            list_drafts,
            save_draft,
            delete_draft,
            save_filter,
            delete_filter,
            apply_filters,
            trust_sender,
            remove_trusted,
            get_close_behavior,
            set_close_behavior,
            get_notify_new_mail,
            set_notify_new_mail,
            open_pending_notification_mail,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, _event| {
            // 窗口聚焦（点通知/Dock 后系统把应用带到前台）→ 提示前端来拉取待打开通知
            if matches!(
                _event,
                tauri::RunEvent::WindowEvent {
                    event: tauri::WindowEvent::Focused(true),
                    ..
                }
            ) {
                watcher::poke_pending_notification_open(_app);
            }
            // macOS：窗口隐藏后点程序坞图标 / 点通知重新打开
            #[cfg(target_os = "macos")]
            {
                // Dock 点击：即使没有待打开通知，也要把隐藏的窗口恢复出来
                if let tauri::RunEvent::Reopen { .. } = _event {
                    watcher::reveal_main_window(_app);
                }
                if matches!(
                    _event,
                    tauri::RunEvent::Reopen { .. } | tauri::RunEvent::Opened { .. }
                ) {
                    watcher::poke_pending_notification_open(_app);
                }
            }
        });
}
