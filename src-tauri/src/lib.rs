pub mod crypto;
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

use models::*;
use serde::Serialize;
use store::{AppState, StoreData};
use tauri::{Manager, State};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppStateView {
    accounts: Vec<Account>,
    identity: IdentityInfo,
    trusted: Vec<TrustedContact>,
    filters: Vec<FilterRule>,
    local_folders: Vec<String>,
}

fn now_date() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn gen_id() -> String {
    let mut b = [0u8; 8];
    let _ = getrandom::getrandom(&mut b);
    hex::encode(b)
}

// ───────────────────────── state / accounts ─────────────────────────

fn identity_info(s: &store::StoreData) -> IdentityInfo {
    IdentityInfo {
        fingerprint: s.active_fingerprint(),
        public_key: s.identity.public_key_b64(),
        created: s.identity.created.clone(),
        mode: s.identity_config.mode.clone(),
        ledger_path: s.identity_config.ledger_path.clone(),
        ledger_address: s.identity_config.ledger_address.clone(),
    }
}

#[tauri::command]
fn get_state(state: State<'_, AppState>) -> Result<AppStateView, String> {
    let s = state.inner.lock().unwrap();
    Ok(AppStateView {
        accounts: s.accounts.clone(),
        identity: identity_info(&s),
        trusted: s.trusted.clone(),
        filters: s.filters.clone(),
        local_folders: s.local_folders.clone(),
    })
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
                .map(|(index, path, address)| LedgerAccountRow { index, path, address })
                .collect()
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn bind_ledger(state: State<'_, AppState>, path: String, address: String) -> Result<IdentityInfo, String> {
    let mut s = state.inner.lock().unwrap();
    s.identity_config = IdentityConfig {
        mode: "ledger".into(),
        ledger_path: Some(path),
        ledger_address: Some(address.to_lowercase()),
    };
    s.save_identity_config()?;
    Ok(identity_info(&s))
}

#[tauri::command]
fn use_local_key(state: State<'_, AppState>) -> Result<IdentityInfo, String> {
    let mut s = state.inner.lock().unwrap();
    s.identity_config = IdentityConfig::default();
    s.save_identity_config()?;
    Ok(identity_info(&s))
}

// ───────────────────────── prefs ─────────────────────────

#[tauri::command]
fn get_close_behavior(state: State<'_, AppState>) -> String {
    state.inner.lock().unwrap().prefs.close_behavior.clone()
}

#[tauri::command]
fn set_close_behavior(state: State<'_, AppState>, behavior: String) -> Result<String, String> {
    if behavior != "hide" && behavior != "quit" {
        return Err(format!("无效的关闭行为: {}", behavior));
    }
    let mut s = state.inner.lock().unwrap();
    s.prefs.close_behavior = behavior.clone();
    s.save_prefs()?;
    Ok(behavior)
}

// ───────────────────────── oauth2 (Microsoft 设备码) ─────────────────────────

#[tauri::command]
async fn oauth_begin_device(client_id: Option<String>) -> Result<oauth::DeviceFlowStart, String> {
    let cid = client_id
        .filter(|c| !c.trim().is_empty())
        .unwrap_or_else(|| oauth::DEFAULT_MS_CLIENT_ID.to_string());
    oauth::begin_device_flow(cid.trim()).await
}

#[tauri::command]
async fn oauth_poll_device(client_id: String, device_code: String) -> Result<oauth::DevicePoll, String> {
    oauth::poll_device(&client_id, &device_code).await
}

/// 取账户凭据；OAuth2 账户的 access_token 临近过期时先刷新并回写 secrets.json
async fn fresh_secret(state: &State<'_, AppState>, account_id: &str) -> Result<AccountSecret, String> {
    let secret = {
        let s = state.inner.lock().unwrap();
        s.secret(account_id)?
    };
    let Some(tokens) = &secret.oauth else { return Ok(secret) };
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
    tauri::async_runtime::spawn_blocking(move || {
        match account.protocol {
            IncomingProtocol::Imap => {
                imap_client::connect(&account, &secret).map(|mut s| {
                    let _ = s.logout();
                })?;
            }
            IncomingProtocol::Pop3 => {
                let mut c = pop3_client::Pop3Client::connect(&account, &secret)?;
                c.quit();
            }
        }
        smtp_client::test_smtp(&account, &secret)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn add_account(
    state: State<'_, AppState>,
    mut account: Account,
    secret: AccountSecret,
) -> Result<Account, String> {
    if account.id.is_empty() {
        account.id = gen_id();
    }
    let (acc2, sec2) = (account.clone(), secret.clone());
    tauri::async_runtime::spawn_blocking(move || match acc2.protocol {
        IncomingProtocol::Imap => imap_client::connect(&acc2, &sec2).map(|mut s| {
            let _ = s.logout();
        }),
        IncomingProtocol::Pop3 => pop3_client::Pop3Client::connect(&acc2, &sec2).map(|mut c| c.quit()),
    })
    .await
    .map_err(|e| e.to_string())??;

    let mut s = state.inner.lock().unwrap();
    s.accounts.retain(|a| a.id != account.id);
    s.accounts.push(account.clone());
    s.secrets.insert(account.id.clone(), secret);
    s.save_accounts()?;
    s.save_secrets()?;
    Ok(account)
}

#[tauri::command]
fn remove_account(state: State<'_, AppState>, account_id: String) -> Result<(), String> {
    let mut s = state.inner.lock().unwrap();
    s.accounts.retain(|a| a.id != account_id);
    s.secrets.remove(&account_id);
    s.save_accounts()?;
    s.save_secrets()
}

// ───────────────────────── folders ─────────────────────────

#[tauri::command]
async fn list_folders(state: State<'_, AppState>, account_id: String) -> Result<Vec<FolderInfo>, String> {
    let (account, local) = {
        let s = state.inner.lock().unwrap();
        (s.account(&account_id)?, s.local_folders.clone())
    };
    let secret = fresh_secret(&state, &account_id).await?;
    match account.protocol {
        IncomingProtocol::Imap => {
            tauri::async_runtime::spawn_blocking(move || imap_client::list_folders(&account, &secret))
                .await
                .map_err(|e| e.to_string())?
        }
        IncomingProtocol::Pop3 => {
            let mut out = vec![FolderInfo { name: "INBOX".into(), display: "收件箱".into() }];
            out.extend(local.into_iter().map(|f| FolderInfo { display: f.clone(), name: f }));
            Ok(out)
        }
    }
}

#[tauri::command]
async fn create_folder(state: State<'_, AppState>, account_id: String, name: String) -> Result<(), String> {
    let account = {
        let s = state.inner.lock().unwrap();
        s.account(&account_id)?
    };
    let secret = fresh_secret(&state, &account_id).await?;
    match account.protocol {
        IncomingProtocol::Imap => {
            tauri::async_runtime::spawn_blocking(move || imap_client::create_folder(&account, &secret, &name))
                .await
                .map_err(|e| e.to_string())?
        }
        IncomingProtocol::Pop3 => {
            let mut s = state.inner.lock().unwrap();
            if !s.local_folders.contains(&name) {
                s.local_folders.push(name);
                s.save_local_folders()?;
            }
            Ok(())
        }
    }
}

// ───────────────────────── messages ─────────────────────────

fn pop_key(account_id: &str, uid: u32) -> String {
    format!("{}/{}", account_id, uid)
}

#[tauri::command]
async fn fetch_messages(
    state: State<'_, AppState>,
    account_id: String,
    folder: String,
    limit: Option<u32>,
) -> Result<Vec<EmailMeta>, String> {
    let limit = limit.unwrap_or(30).min(100);
    let (account, trusted) = {
        let s = state.inner.lock().unwrap();
        let account = s.account(&account_id)?;
        let trusted = s.trusted_for_verify(&account);
        (account, trusted)
    };
    let secret = fresh_secret(&state, &account_id).await?;

    match account.protocol {
        IncomingProtocol::Imap => {
            let folder2 = folder.clone();
            let acc2 = account.clone();
            let raws = tauri::async_runtime::spawn_blocking(move || {
                imap_client::fetch_window(&acc2, &secret, &folder2, limit)
            })
            .await
            .map_err(|e| e.to_string())??;

            let mut metas = Vec::new();
            let mut s = state.inner.lock().unwrap();
            for r in raws {
                if let Ok(full) = mail::parse_email(&r.raw, r.uid, &account_id, &folder, r.unread, &trusted) {
                    metas.push(full.meta.clone());
                    s.mail_cache.insert(StoreData::cache_key(&account_id, &folder, r.uid), full);
                }
            }
            Ok(metas)
        }
        IncomingProtocol::Pop3 => {
            let acc2 = account.clone();
            let raws = tauri::async_runtime::spawn_blocking(move || {
                pop3_client::fetch_window(&acc2, &secret, limit)
            })
            .await
            .map_err(|e| e.to_string())??;

            let mut metas = Vec::new();
            let mut s = state.inner.lock().unwrap();
            for r in raws {
                let key = pop_key(&account_id, r.seq);
                let assigned = s.local_assign.get(&key).cloned().unwrap_or_else(|| "INBOX".into());
                if assigned != folder {
                    continue;
                }
                let unread = !s.local_read.contains(&key);
                if let Ok(full) = mail::parse_email(&r.raw, r.seq, &account_id, &folder, unread, &trusted) {
                    metas.push(full.meta.clone());
                    s.mail_cache.insert(StoreData::cache_key(&account_id, &folder, r.seq), full);
                }
            }
            Ok(metas)
        }
    }
}

#[tauri::command]
fn get_message(
    state: State<'_, AppState>,
    account_id: String,
    folder: String,
    uid: u32,
) -> Result<EmailFull, String> {
    let s = state.inner.lock().unwrap();
    s.mail_cache
        .get(&StoreData::cache_key(&account_id, &folder, uid))
        .cloned()
        .ok_or_else(|| "邮件不在缓存中，请刷新列表".into())
}

#[tauri::command]
async fn move_message(
    state: State<'_, AppState>,
    account_id: String,
    folder: String,
    uid: u32,
    target: String,
) -> Result<(), String> {
    let account = {
        let s = state.inner.lock().unwrap();
        s.account(&account_id)?
    };
    let secret = fresh_secret(&state, &account_id).await?;
    match account.protocol {
        IncomingProtocol::Imap => {
            let (f2, t2) = (folder.clone(), target.clone());
            tauri::async_runtime::spawn_blocking(move || {
                imap_client::move_message(&account, &secret, &f2, uid, &t2)
            })
            .await
            .map_err(|e| e.to_string())??;
        }
        IncomingProtocol::Pop3 => {
            let mut s = state.inner.lock().unwrap();
            let key = pop_key(&account_id, uid);
            if target == "INBOX" {
                s.local_assign.remove(&key);
            } else {
                s.local_assign.insert(key, target.clone());
            }
            s.save_local_folders()?;
        }
    }
    let mut s = state.inner.lock().unwrap();
    if let Some(mut full) = s.mail_cache.remove(&StoreData::cache_key(&account_id, &folder, uid)) {
        full.meta.folder = target.clone();
        s.mail_cache.insert(StoreData::cache_key(&account_id, &target, uid), full);
    }
    Ok(())
}

#[tauri::command]
async fn set_read(
    state: State<'_, AppState>,
    account_id: String,
    folder: String,
    uid: u32,
    read: bool,
) -> Result<(), String> {
    let account = {
        let s = state.inner.lock().unwrap();
        s.account(&account_id)?
    };
    let secret = fresh_secret(&state, &account_id).await?;
    match account.protocol {
        IncomingProtocol::Imap => {
            let f2 = folder.clone();
            tauri::async_runtime::spawn_blocking(move || {
                imap_client::set_read(&account, &secret, &f2, uid, read)
            })
            .await
            .map_err(|e| e.to_string())??;
        }
        IncomingProtocol::Pop3 => {
            let mut s = state.inner.lock().unwrap();
            let key = pop_key(&account_id, uid);
            if read && !s.local_read.contains(&key) {
                s.local_read.push(key);
            } else if !read {
                s.local_read.retain(|k| k != &key);
            }
            s.save_local_folders()?;
        }
    }
    let mut s = state.inner.lock().unwrap();
    if let Some(full) = s.mail_cache.get_mut(&StoreData::cache_key(&account_id, &folder, uid)) {
        full.meta.unread = !read;
    }
    Ok(())
}

#[tauri::command]
async fn delete_message(
    state: State<'_, AppState>,
    account_id: String,
    folder: String,
    uid: u32,
) -> Result<(), String> {
    let account = {
        let s = state.inner.lock().unwrap();
        s.account(&account_id)?
    };
    let secret = fresh_secret(&state, &account_id).await?;
    match account.protocol {
        IncomingProtocol::Imap => {
            let f2 = folder.clone();
            tauri::async_runtime::spawn_blocking(move || {
                imap_client::delete_message(&account, &secret, &f2, uid)
            })
            .await
            .map_err(|e| e.to_string())??;
        }
        IncomingProtocol::Pop3 => {
            tauri::async_runtime::spawn_blocking(move || {
                pop3_client::delete_message(&account, &secret, uid)
            })
            .await
            .map_err(|e| e.to_string())??;
        }
    }
    let mut s = state.inner.lock().unwrap();
    s.mail_cache.remove(&StoreData::cache_key(&account_id, &folder, uid));
    Ok(())
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
) -> Result<smtp_client::SendResult, String> {
    let (account, key_bytes, created, id_cfg) = {
        let s = state.inner.lock().unwrap();
        (
            s.account(&account_id)?,
            s.identity.signing_key.to_bytes(),
            s.identity.created.clone(),
            s.identity_config.clone(),
        )
    };
    let secret = fresh_secret(&state, &account_id).await?;
    tauri::async_runtime::spawn_blocking(move || {
        let identity = crypto::Identity {
            signing_key: ed25519_dalek::SigningKey::from_bytes(&key_bytes),
            created,
        };
        let signer = if !sign {
            smtp_client::Signer::None
        } else if id_cfg.mode == "ledger" {
            let path = id_cfg.ledger_path.ok_or("Ledger 未绑定派生路径，请在「身份与密钥」重新绑定")?;
            let address = id_cfg.ledger_address.ok_or("Ledger 未绑定地址，请在「身份与密钥」重新绑定")?;
            smtp_client::Signer::Ledger { path, address }
        } else {
            smtp_client::Signer::Local(&identity)
        };
        smtp_client::send_mail(&account, &secret, signer, to, cc, &subject, &body)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ───────────────────────── filters ─────────────────────────

#[tauri::command]
fn save_filter(state: State<'_, AppState>, mut rule: FilterRule) -> Result<Vec<FilterRule>, String> {
    let mut s = state.inner.lock().unwrap();
    if rule.id.is_empty() {
        rule.id = gen_id();
    }
    s.filters.retain(|f| f.id != rule.id);
    s.filters.push(rule);
    s.save_filters()?;
    Ok(s.filters.clone())
}

#[tauri::command]
fn delete_filter(state: State<'_, AppState>, id: String) -> Result<Vec<FilterRule>, String> {
    let mut s = state.inner.lock().unwrap();
    s.filters.retain(|f| f.id != id);
    s.save_filters()?;
    Ok(s.filters.clone())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplyResult {
    moved: u32,
    details: Vec<String>,
}

/// 对收件箱执行所有过滤规则：匹配则移动到目标目录
#[tauri::command]
async fn apply_filters(state: State<'_, AppState>, account_id: String) -> Result<ApplyResult, String> {
    // 先拉取收件箱（写入缓存）
    let metas = fetch_messages(state.clone(), account_id.clone(), "INBOX".into(), Some(50)).await?;
    let (rules, mails) = {
        let s = state.inner.lock().unwrap();
        let mails: Vec<EmailFull> = metas
            .iter()
            .filter_map(|m| {
                s.mail_cache
                    .get(&StoreData::cache_key(&account_id, "INBOX", m.uid))
                    .cloned()
            })
            .collect();
        (s.filters.clone(), mails)
    };
    let mut moved = 0u32;
    let mut details = Vec::new();
    for mail in &mails {
        if let Some(rule) = rules.iter().find(|r| filters::rule_matches(r, mail)) {
            move_message(
                state.clone(),
                account_id.clone(),
                "INBOX".into(),
                mail.meta.uid,
                rule.target_folder.clone(),
            )
            .await?;
            if rule.mark_read {
                let _ = set_read(state.clone(), account_id.clone(), rule.target_folder.clone(), mail.meta.uid, true).await;
            }
            details.push(format!("「{}」→ {}", mail.meta.subject, rule.target_folder));
            moved += 1;
        }
    }
    Ok(ApplyResult { moved, details })
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
    s.trusted.retain(|t| !t.email.eq_ignore_ascii_case(&email));
    s.trusted.push(TrustedContact {
        name,
        email,
        fingerprint,
        org,
        since: now_date(),
        verified_count: 1,
    });
    s.save_trusted()?;
    Ok(s.trusted.clone())
}

#[tauri::command]
fn remove_trusted(state: State<'_, AppState>, email: String) -> Result<Vec<TrustedContact>, String> {
    let mut s = state.inner.lock().unwrap();
    s.trusted.retain(|t| !t.email.eq_ignore_ascii_case(&email));
    s.save_trusted()?;
    Ok(s.trusted.clone())
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
    let mut builder = tauri::Builder::default().plugin(tauri_plugin_opener::init());
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
            let dir = app
                .path()
                .app_config_dir()
                .expect("无法获取应用配置目录");
            let data = StoreData::load(dir).expect("初始化本地存储失败");
            app.manage(AppState { inner: std::sync::Mutex::new(data) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            check_for_update,
            oauth_begin_device,
            oauth_poll_device,
            ledger_get_addresses,
            bind_ledger,
            use_local_key,
            test_connection,
            add_account,
            remove_account,
            list_folders,
            create_folder,
            fetch_messages,
            get_message,
            move_message,
            set_read,
            delete_message,
            send_mail,
            save_filter,
            delete_filter,
            apply_filters,
            trust_sender,
            remove_trusted,
            get_close_behavior,
            set_close_behavior,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, _event| {
            // macOS：窗口隐藏后点程序坞图标重新打开
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = _event {
                if let Some(window) = _app.webview_windows().values().next() {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        });
}
