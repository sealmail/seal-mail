pub mod cli;
pub mod core;
pub mod crypto;
pub mod db;
pub mod filters;
pub mod i18n;
pub mod imap_client;
pub mod ledger;
pub mod logging;
#[cfg(target_os = "macos")]
pub mod mac_notify;
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
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

fn gen_id() -> String {
    let mut b = [0u8; 8];
    let _ = getrandom::getrandom(&mut b);
    hex::encode(b)
}

/// 后台 meta 回填是否在跑；已在跑时再请求只标记 dirty，本轮结束后再扫一遍。
static META_BACKFILL_RUNNING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static META_BACKFILL_DIRTY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// 启动/重启后台补全 meta_json。列表缺 meta 时先显示「…」占位；
/// 本任务小批解析并写回，同时 emit `meta-cache-updated` 让前端刷新列表。
/// 可重复调用：可信联系人变更清缓存后、同步后发现缺口时都应唤醒。
fn spawn_meta_backfill(app: AppHandle) {
    use std::sync::atomic::Ordering;
    META_BACKFILL_DIRTY.store(true, Ordering::SeqCst);
    if META_BACKFILL_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        // 已有 worker 在跑，仅标记 dirty，由它收尾后再扫
        return;
    }
    std::thread::spawn(move || {
        // 等首屏 list_cached 先出来，避免和启动 IO 抢锁
        std::thread::sleep(std::time::Duration::from_millis(800));
        let state = app.state::<AppState>();
        loop {
            META_BACKFILL_DIRTY.store(false, Ordering::SeqCst);
            let targets = {
                let s = state.inner.lock().unwrap();
                db::missing_meta_targets(&s.db).unwrap_or_default()
            };
            if targets.is_empty() {
                if !META_BACKFILL_DIRTY.swap(false, Ordering::SeqCst) {
                    META_BACKFILL_RUNNING.store(false, Ordering::SeqCst);
                    // 退出前再看一眼：若刚好有人 dirty，重新接棒
                    if META_BACKFILL_DIRTY.load(Ordering::SeqCst)
                        && META_BACKFILL_RUNNING
                            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                            .is_ok()
                    {
                        continue;
                    }
                    return;
                }
                continue;
            }
            logging::log(format!(
                "[cache] 后台补全 meta 缓存开始：{} 个目录",
                targets.len()
            ));
            let t0 = std::time::Instant::now();
            let mut total = 0u32;
            for (account_id, folder) in targets {
                let mut cursor = 0u32;
                loop {
                    let batch = {
                        let mut s = state.inner.lock().unwrap();
                        core::backfill_meta_batch(&mut s, &account_id, &folder, cursor, 40)
                    };
                    match batch {
                        Ok((0, _)) => break,
                        Ok((n, max_uid)) => {
                            total += n;
                            // 每批通知前端刷新占位行（防列表长期卡在「…」）
                            let _ = app.emit(
                                "meta-cache-updated",
                                serde_json::json!({
                                    "accountId": account_id,
                                    "folder": folder,
                                    "filled": n,
                                }),
                            );
                            match max_uid {
                                Some(u) => cursor = u,
                                None => break,
                            }
                        }
                        Err(_) => break, // 账户已删除等：跳过该目录
                    }
                    // 让出状态锁和磁盘 IO
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
            }
            logging::log(format!(
                "[cache] 后台补全 meta 缓存完成：{} 封，耗时 {}ms",
                total,
                t0.elapsed().as_millis()
            ));
            // 完成一轮后再看 dirty（信任列表变化 / 新同步）是否需要重扫
            if !META_BACKFILL_DIRTY.swap(false, Ordering::SeqCst) {
                META_BACKFILL_RUNNING.store(false, Ordering::SeqCst);
                if META_BACKFILL_DIRTY.load(Ordering::SeqCst)
                    && META_BACKFILL_RUNNING
                        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok()
                {
                    continue;
                }
                return;
            }
        }
    });
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

/// GUI 全部业务调用都经过这里。必须是 async：Tauri 2 的同步命令在主线程执行，
/// 之前每次等待 CLI 子进程都会冻结 UI 事件循环（同步邮件时整窗卡死 2 秒以上）；
/// async + spawn_blocking 让调用在线程池并行，主线程始终响应。
#[tauri::command]
async fn cli_json(
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
    // 记录命令与耗时，真机排查性能/失败时直接看日志
    let t0 = std::time::Instant::now();
    let brief = args
        .iter()
        .take_while(|a| !a.starts_with("--"))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    let app2 = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || cli_json_inner(app, args, stdin, env))
        .await
        .map_err(|e| format!("CLI 任务执行失败: {e}"))?;
    logging::log(format!(
        "[perf] cli {brief} {}ms ok={}",
        t0.elapsed().as_millis(),
        result.is_ok()
    ));
    // 偏好由 CLI 子进程写盘：成功后把 GUI 常驻进程的内存态一并刷新，
    // 否则 close_behavior/notify/language 要重启才生效
    if result.is_ok() && brief.starts_with("pref") {
        refresh_prefs_in_memory(&app2);
    }
    result
}

fn refresh_prefs_in_memory(app: &AppHandle) {
    let Ok(dir) = app.path().app_config_dir() else {
        return;
    };
    let prefs = store::StoreData::load_prefs(&dir);
    crate::i18n::set_lang_from_pref(&prefs.language);
    if let Some(state) = app.try_state::<AppState>() {
        state.inner.lock().unwrap().prefs = prefs;
    }
}

fn cli_json_inner(
    app: AppHandle,
    args: Vec<String>,
    stdin: Option<String>,
    env: Option<HashMap<String, String>>,
) -> Result<serde_json::Value, String> {
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
        logging::log(format!("[notify] 前端取走待打开目标 {target:?}"));
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
    force_refresh_secret(state, account_id).await
}

/// 无视本地过期时间，强制刷新 OAuth 令牌并回写 secrets.json。
/// 用于服务器拒绝认证（token 被提前作废）时的兜底；非 OAuth 账户原样返回。
async fn force_refresh_secret(
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
    let refreshed = oauth::refresh_tokens(tokens).await?;
    let mut s = state.inner.lock().unwrap();
    let mut updated = s
        .secrets
        .get(account_id)
        .cloned()
        .ok_or_else(|| format!("账户密码缺失: {}", account_id))?;
    updated.oauth = Some(refreshed);
    s.update_secret(account_id, updated.clone())?;
    Ok(updated)
}

/// 用最新凭据执行网络操作；OAuth2 认证被服务器拒绝时强制刷新令牌并重试一次。
/// 覆盖 access_token 被提前作废（改密码/撤销授权/安全事件）而本地 expires_at 未到期的场景，
/// 此时 fresh_secret 的按时刷新永远不会触发，只能靠认证失败信号驱动。
async fn with_oauth_retry<T>(
    state: &State<'_, AppState>,
    account_id: &str,
    op: impl Fn(&AccountSecret) -> Result<T, String>,
) -> Result<T, String> {
    let secret = fresh_secret(state, account_id).await?;
    let has_oauth = secret.oauth.is_some();
    match op(&secret) {
        Err(e) if has_oauth && oauth::is_auth_rejected(&e) => {
            logging::log(format!(
                "[oauth] {} 认证被拒，强制刷新令牌后重试: {}",
                account_id, e
            ));
            let secret = force_refresh_secret(state, account_id).await?;
            op(&secret)
        }
        other => other,
    }
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
    with_oauth_retry(&state, &account_id, |_| {
        let s = state.inner.lock().unwrap();
        core::list_folders(&s, &account_id)
    })
    .await
}

#[tauri::command]
async fn create_folder(
    state: State<'_, AppState>,
    account_id: String,
    name: String,
) -> Result<(), String> {
    with_oauth_retry(&state, &account_id, |_| {
        let mut s = state.inner.lock().unwrap();
        core::create_folder(&mut s, &account_id, name.clone())
    })
    .await
}

#[tauri::command]
async fn delete_folder(
    state: State<'_, AppState>,
    account_id: String,
    name: String,
) -> Result<(), String> {
    with_oauth_retry(&state, &account_id, |_| {
        let mut s = state.inner.lock().unwrap();
        core::delete_folder(&mut s, &account_id, &name)
    })
    .await
}

// ───────────────────────── messages ─────────────────────────

/// 从本地缓存读列表（秒出、可离线）。缺 meta 时返回「…」占位，并唤醒后台回填。
#[tauri::command]
fn list_cached(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    folder: String,
    offset: u32,
    limit: u32,
) -> Result<core::CachedList, String> {
    let mut s = state.inner.lock().unwrap();
    let list = core::list_cached(&mut s, &account_id, &folder, offset, limit)?;
    let has_stub = list.metas.iter().any(core::is_stub_meta);
    drop(s);
    if has_stub {
        spawn_meta_backfill(app);
    }
    Ok(list)
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
    let result = with_oauth_retry(&state, &account_id, |secret| {
        let mut s = state.inner.lock().unwrap();
        core::sync_messages(&mut s, &account_id, &folder, secret)
    })
    .await?;
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
    let result = with_oauth_retry(&state, &account_id, |secret| {
        let mut s = state.inner.lock().unwrap();
        core::sync_older_messages(&mut s, &account_id, &folder, secret)
    })
    .await?;
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
    with_oauth_retry(&state, &account_id, |secret| {
        let mut s = state.inner.lock().unwrap();
        core::move_message(&mut s, &account_id, &folder, uid, &target, secret)
    })
    .await
}

/// 一键归档：IMAP 移入服务器归档目录；POP3 移入本地「归档」虚拟目录。
#[tauri::command]
async fn archive_message(
    state: State<'_, AppState>,
    account_id: String,
    folder: String,
    uid: u32,
) -> Result<(), String> {
    with_oauth_retry(&state, &account_id, |secret| {
        let mut s = state.inner.lock().unwrap();
        core::archive_message(&mut s, &account_id, &folder, uid, secret)
    })
    .await
}

#[tauri::command]
async fn set_read(
    state: State<'_, AppState>,
    account_id: String,
    folder: String,
    uid: u32,
    read: bool,
) -> Result<(), String> {
    with_oauth_retry(&state, &account_id, |secret| {
        let mut s = state.inner.lock().unwrap();
        core::set_read(&mut s, &account_id, &folder, &[uid], read, secret)
    })
    .await
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
    with_oauth_retry(&state, &account_id, |secret| {
        let mut s = state.inner.lock().unwrap();
        core::set_read(&mut s, &account_id, &folder, &uids, read, secret)
    })
    .await
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
    with_oauth_retry(&state, &account_id, |secret| {
        let mut s = state.inner.lock().unwrap();
        core::set_flagged(&mut s, &account_id, &folder, uid, flagged, secret)
    })
    .await
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
    with_oauth_retry(&state, &account_id, |secret| {
        let mut s = state.inner.lock().unwrap();
        core::delete_message(&mut s, &account_id, &folder, uid, permanent, secret)
    })
    .await
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
    if cached {
        let s = state.inner.lock().unwrap();
        core::save_attachment(&s, &account_id, &folder, uid, index, &path, None).map(|_| ())
    } else {
        with_oauth_retry(&state, &account_id, |secret| {
            let s = state.inner.lock().unwrap();
            core::save_attachment(&s, &account_id, &folder, uid, index, &path, Some(secret))
                .map(|_| ())
        })
        .await
    }
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
    with_oauth_retry(&state, &account_id, |secret| {
        let mut s = state.inner.lock().unwrap();
        core::send_mail(
            &mut s,
            &account_id,
            secret,
            to.clone(),
            cc.clone(),
            &subject,
            &body,
            sign,
            files.clone(),
        )
    })
    .await
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
    with_oauth_retry(&state, &account_id, |secret| {
        let mut s = state.inner.lock().unwrap();
        core::apply_filters(&mut s, &account_id, secret)
    })
    .await
}

// ───────────────────────── trust ─────────────────────────

#[tauri::command]
fn trust_sender(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    email: String,
    fingerprint: String,
    org: Option<String>,
) -> Result<Vec<TrustedContact>, String> {
    let mut s = state.inner.lock().unwrap();
    let list = core::trust_sender(&mut s, name, email, fingerprint, org)?;
    drop(s);
    // 信任关系变化会清空 meta_json，立刻唤醒后台回填，避免列表长期「…」
    spawn_meta_backfill(app);
    Ok(list)
}

#[tauri::command]
fn remove_trusted(
    app: AppHandle,
    state: State<'_, AppState>,
    email: String,
) -> Result<Vec<TrustedContact>, String> {
    let mut s = state.inner.lock().unwrap();
    let list = core::remove_trusted(&mut s, email)?;
    drop(s);
    spawn_meta_backfill(app);
    Ok(list)
}

// ───────────────────────── update ─────────────────────────

/// 签名自动升级不可用时的回退：直接查 GitHub Releases，引导手动下载
#[tauri::command]
async fn check_for_update() -> Result<updater::UpdateInfo, String> {
    updater::check_for_update().await
}

// ───────────────────────── entry ─────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// macOS 专用：在默认菜单的 View 子菜单里插入 放大/缩小/实际大小（Cmd+= / Cmd+- / Cmd+0），
/// 触发后发事件给当前聚焦窗口，由前端 useZoomShortcuts 统一应用缩放。
#[cfg(target_os = "macos")]
fn setup_zoom_menu(app: &tauri::App) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItemBuilder, MenuItemKind, PredefinedMenuItem, SubmenuBuilder};
    use tauri::Emitter;

    let menu = Menu::default(app.handle())?;
    let zoom_reset = MenuItemBuilder::with_id("zoom-reset", "实际大小")
        .accelerator("Cmd+0")
        .build(app)?;
    let zoom_in = MenuItemBuilder::with_id("zoom-in", "放大")
        .accelerator("Cmd+=")
        .build(app)?;
    let zoom_out = MenuItemBuilder::with_id("zoom-out", "缩小")
        .accelerator("Cmd+-")
        .build(app)?;
    let separator = PredefinedMenuItem::separator(app)?;

    let existing_view = menu.items()?.into_iter().find_map(|item| match item {
        MenuItemKind::Submenu(s) if s.text().map(|t| t == "View").unwrap_or(false) => Some(s),
        _ => None,
    });
    match existing_view {
        Some(view) => {
            view.insert_items(&[&zoom_reset, &zoom_in, &zoom_out, &separator], 0)?;
        }
        None => {
            let view = SubmenuBuilder::new(app, "显示")
                .item(&zoom_reset)
                .item(&zoom_in)
                .item(&zoom_out)
                .build()?;
            menu.append(&view)?;
        }
    }
    app.set_menu(menu)?;

    app.on_menu_event(|app, event| {
        let payload = match event.id().as_ref() {
            "zoom-in" => serde_json::json!({ "kind": "step", "delta": 0.1 }),
            "zoom-out" => serde_json::json!({ "kind": "step", "delta": -0.1 }),
            "zoom-reset" => serde_json::json!({ "kind": "reset" }),
            _ => return,
        };
        if let Some(win) = app
            .webview_windows()
            .into_values()
            .find(|w| w.is_focused().unwrap_or(false))
        {
            let _ = win.emit_to(win.label(), "sealmail-menu-zoom", payload);
        }
    });
    Ok(())
}

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
            logging::init(&dir);
            let data = StoreData::load(dir).expect("初始化本地存储失败");
            app.manage(AppState {
                inner: std::sync::Mutex::new(data),
            });
            // macOS：界面缩放挂到原生菜单加速键上。切换 app 回来后 WKWebView 可能
            // 丢失 first responder（页面收不到 Cmd+/-），原生菜单不依赖 webview 焦点。
            #[cfg(target_os = "macos")]
            setup_zoom_menu(app)?;
            // 启动新邮件监听（IMAP IDLE / POP3 轮询）
            watcher::ensure_watchers(&app.handle().clone());
            watcher::debug_fire_test_notification(&app.handle().clone());
            spawn_meta_backfill(app.handle().clone());
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
