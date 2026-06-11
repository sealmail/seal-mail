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

#[tauri::command]
fn get_notify_new_mail(state: State<'_, AppState>) -> bool {
    state.inner.lock().unwrap().prefs.notify_new_mail
}

#[tauri::command]
fn set_notify_new_mail(state: State<'_, AppState>, enabled: bool) -> Result<bool, String> {
    let mut s = state.inner.lock().unwrap();
    s.prefs.notify_new_mail = enabled;
    s.save_prefs()?;
    Ok(enabled)
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
    app: tauri::AppHandle,
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

    {
        let mut s = state.inner.lock().unwrap();
        s.accounts.retain(|a| a.id != account.id);
        s.accounts.push(account.clone());
        s.secrets.insert(account.id.clone(), secret);
        s.save_accounts()?;
        s.save_secrets()?;
    }
    watcher::ensure_watchers(&app);
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
            // POP3 无服务器目录：内置一个本地「已删除」虚拟目录承接软删除
            let mut out = vec![
                FolderInfo { name: "INBOX".into(), display: "收件箱".into(), role: None },
                FolderInfo { name: POP3_TRASH.into(), display: "已删除".into(), role: Some("trash".into()) },
            ];
            out.extend(
                local
                    .into_iter()
                    .filter(|f| f != POP3_TRASH)
                    .map(|f| FolderInfo { display: f.clone(), name: f, role: None }),
            );
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

/// POP3 本地虚拟回收站目录名
const POP3_TRASH: &str = "已删除";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CachedList {
    metas: Vec<EmailMeta>,
    total: i64,
}

/// 从本地缓存读列表（秒出、可离线）。读取时重新解析+验证，信任列表变化即时生效。
#[tauri::command]
fn list_cached(
    state: State<'_, AppState>,
    account_id: String,
    folder: String,
    offset: u32,
    limit: u32,
) -> Result<CachedList, String> {
    let mut s = state.inner.lock().unwrap();
    let account = s.account(&account_id)?;
    let trusted = s.trusted_for_verify(&account);
    let rows = db::list(&s.db, &account_id, &folder, offset, limit.min(200))?;
    let total = db::count(&s.db, &account_id, &folder)?;
    let mut metas = Vec::new();
    for r in rows {
        match mail::parse_email(&r.raw, r.uid, &account_id, &folder, r.unread, r.flagged, &trusted) {
            Ok(full) => {
                metas.push(full.meta.clone());
                s.mail_cache.insert(StoreData::cache_key(&account_id, &folder, r.uid), full);
            }
            Err(e) => eprintln!("[cache] 解析缓存邮件失败 uid={}: {}", r.uid, e),
        }
    }
    Ok(CachedList { metas, total })
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
    let (account, trusted) = {
        let s = state.inner.lock().unwrap();
        let account = s.account(&account_id)?;
        let trusted = s.trusted_for_verify(&account);
        (account, trusted)
    };
    let secret = fresh_secret(&state, &account_id).await?;

    match account.protocol {
        IncomingProtocol::Imap => {
            let (validity, max_uid, low) = {
                let s = state.inner.lock().unwrap();
                (
                    db::uidvalidity(&s.db, &account_id, &folder)?,
                    db::max_uid(&s.db, &account_id, &folder)?,
                    db::window_low(&s.db, &account_id, &folder, db::FLAG_SYNC_WINDOW)?,
                )
            };
            let (acc2, f2) = (account.clone(), folder.clone());
            let sf = tauri::async_runtime::spawn_blocking(move || {
                imap_client::sync_fetch(&acc2, &secret, &f2, validity, max_uid, low, db::INITIAL_WINDOW)
            })
            .await
            .map_err(|e| e.to_string())??;

            let mut s = state.inner.lock().unwrap();
            if sf.reset {
                db::clear_folder(&s.db, &account_id, &folder)?;
            }
            db::set_uidvalidity(&s.db, &account_id, &folder, sf.uidvalidity)?;
            let mut added = 0u32;
            for m in &sf.new_mails {
                let ts = match mail::parse_email(&m.raw, m.uid, &account_id, &folder, m.unread, m.flagged, &trusted) {
                    Ok(full) => {
                        s.upsert_contact(&full.meta.from_name, &full.meta.from_addr, full.meta.timestamp);
                        full.meta.timestamp
                    }
                    Err(_) => 0,
                };
                db::upsert_message(&s.db, &account_id, &folder, m.uid, None, m.unread, m.flagged, ts, &m.raw)?;
                added += 1;
            }
            if !sf.reset {
                let server: std::collections::HashMap<u32, (bool, bool)> =
                    sf.server_flags.iter().map(|(u, a, b)| (*u, (*a, *b))).collect();
                for uid in db::uids_from(&s.db, &account_id, &folder, sf.flags_low)? {
                    match server.get(&uid) {
                        Some((unread, flagged)) => {
                            db::update_flags(&s.db, &account_id, &folder, uid, *unread, *flagged)?
                        }
                        None => {
                            // 服务器上已不存在（被其他客户端删除/移动）
                            db::delete_row(&s.db, &account_id, &folder, uid)?;
                            s.mail_cache.remove(&StoreData::cache_key(&account_id, &folder, uid));
                        }
                    }
                }
            }
            if let Err(e) = s.save_contacts() {
                eprintln!("[contacts] 保存失败: {}", e);
            }
            Ok(SyncResult { added, total: db::count(&s.db, &account_id, &folder)? })
        }
        IncomingProtocol::Pop3 => {
            let known: std::collections::HashSet<String> = {
                let s = state.inner.lock().unwrap();
                db::pop_known_uidls(&s.db, &account_id)?
                    .into_iter()
                    .map(|(uidl, _, _)| uidl)
                    .collect()
            };
            let acc2 = account.clone();
            let known2 = known.clone();
            let ps = tauri::async_runtime::spawn_blocking(move || {
                pop3_client::sync_fetch(&acc2, &secret, &known2, db::INITIAL_WINDOW)
            })
            .await
            .map_err(|e| e.to_string())??;

            let mut s = state.inner.lock().unwrap();
            let mut added = 0u32;
            for (uidl, raw) in &ps.new_mails {
                let uid = db::pop_next_uid(&s.db, &account_id)?;
                let ts = match mail::parse_email(raw, uid, &account_id, "INBOX", true, false, &trusted) {
                    Ok(full) => {
                        s.upsert_contact(&full.meta.from_name, &full.meta.from_addr, full.meta.timestamp);
                        full.meta.timestamp
                    }
                    Err(_) => 0,
                };
                db::upsert_message(&s.db, &account_id, "INBOX", uid, Some(uidl), true, false, ts, raw)?;
                added += 1;
            }
            // 服务器侧删除检测（跨所有本地目录）
            let server: std::collections::HashSet<&String> = ps.all_uidls.iter().collect();
            for (uidl, fld, uid) in db::pop_known_uidls(&s.db, &account_id)? {
                if !server.contains(&uidl) {
                    db::delete_row(&s.db, &account_id, &fld, uid)?;
                    s.mail_cache.remove(&StoreData::cache_key(&account_id, &fld, uid));
                }
            }
            if let Err(e) = s.save_contacts() {
                eprintln!("[contacts] 保存失败: {}", e);
            }
            Ok(SyncResult { added, total: db::count(&s.db, &account_id, &folder)? })
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
    let mut s = state.inner.lock().unwrap();
    let key = StoreData::cache_key(&account_id, &folder, uid);
    if let Some(full) = s.mail_cache.get(&key) {
        return Ok(full.clone());
    }
    let account = s.account(&account_id)?;
    let trusted = s.trusted_for_verify(&account);
    let row = db::get_raw(&s.db, &account_id, &folder, uid)?
        .ok_or("邮件不在本地缓存中，请刷新列表")?;
    let full = mail::parse_email(&row.raw, uid, &account_id, &folder, row.unread, row.flagged, &trusted)?;
    s.mail_cache.insert(key, full.clone());
    Ok(full)
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
            // 服务器移动后邮件在目标目录会拿到新 UID，本地行删除，目标目录下次同步补齐
            let mut s = state.inner.lock().unwrap();
            db::delete_row(&s.db, &account_id, &folder, uid)?;
            s.mail_cache.remove(&StoreData::cache_key(&account_id, &folder, uid));
        }
        IncomingProtocol::Pop3 => {
            // POP3 目录纯本地：直接改归属
            let mut s = state.inner.lock().unwrap();
            db::set_folder(&s.db, &account_id, &folder, uid, &target)?;
            if let Some(mut full) = s.mail_cache.remove(&StoreData::cache_key(&account_id, &folder, uid)) {
                full.meta.folder = target.clone();
                s.mail_cache.insert(StoreData::cache_key(&account_id, &target, uid), full);
            }
        }
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
    if account.protocol == IncomingProtocol::Imap {
        let f2 = folder.clone();
        tauri::async_runtime::spawn_blocking(move || {
            imap_client::set_read(&account, &secret, &f2, uid, read)
        })
        .await
        .map_err(|e| e.to_string())??;
    }
    let mut s = state.inner.lock().unwrap();
    db::set_unread(&s.db, &account_id, &folder, &[uid], !read)?;
    if let Some(full) = s.mail_cache.get_mut(&StoreData::cache_key(&account_id, &folder, uid)) {
        full.meta.unread = !read;
    }
    Ok(())
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
    let account = {
        let s = state.inner.lock().unwrap();
        s.account(&account_id)?
    };
    let secret = fresh_secret(&state, &account_id).await?;
    if account.protocol == IncomingProtocol::Imap {
        let (f2, u2) = (folder.clone(), uids.clone());
        tauri::async_runtime::spawn_blocking(move || {
            imap_client::set_read_many(&account, &secret, &f2, &u2, read)
        })
        .await
        .map_err(|e| e.to_string())??;
    }
    let mut s = state.inner.lock().unwrap();
    db::set_unread(&s.db, &account_id, &folder, &uids, !read)?;
    for uid in &uids {
        if let Some(full) = s.mail_cache.get_mut(&StoreData::cache_key(&account_id, &folder, *uid)) {
            full.meta.unread = !read;
        }
    }
    Ok(())
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
    let account = {
        let s = state.inner.lock().unwrap();
        s.account(&account_id)?
    };
    if account.protocol == IncomingProtocol::Imap {
        let secret = fresh_secret(&state, &account_id).await?;
        let f2 = folder.clone();
        tauri::async_runtime::spawn_blocking(move || {
            imap_client::set_flagged(&account, &secret, &f2, uid, flagged)
        })
        .await
        .map_err(|e| e.to_string())??;
    }
    let mut s = state.inner.lock().unwrap();
    db::set_flagged(&s.db, &account_id, &folder, uid, flagged)?;
    if let Some(full) = s.mail_cache.get_mut(&StoreData::cache_key(&account_id, &folder, uid)) {
        full.meta.flagged = flagged;
    }
    Ok(())
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
    let account = {
        let s = state.inner.lock().unwrap();
        s.account(&account_id)?
    };
    match account.protocol {
        IncomingProtocol::Imap => {
            let secret = fresh_secret(&state, &account_id).await?;
            let f2 = folder.clone();
            tauri::async_runtime::spawn_blocking(move || {
                imap_client::delete_message(&account, &secret, &f2, uid, permanent)
            })
            .await
            .map_err(|e| e.to_string())??;
            let mut s = state.inner.lock().unwrap();
            db::delete_row(&s.db, &account_id, &folder, uid)?;
            s.mail_cache.remove(&StoreData::cache_key(&account_id, &folder, uid));
        }
        IncomingProtocol::Pop3 => {
            if permanent {
                let uidl = {
                    let s = state.inner.lock().unwrap();
                    db::pop_uidl_of(&s.db, &account_id, &folder, uid)?
                        .ok_or("本地缓存缺少该邮件的服务器标识，请刷新后重试")?
                };
                let secret = fresh_secret(&state, &account_id).await?;
                tauri::async_runtime::spawn_blocking(move || {
                    pop3_client::delete_by_uidl(&account, &secret, &uidl)
                })
                .await
                .map_err(|e| e.to_string())??;
                let mut s = state.inner.lock().unwrap();
                db::delete_row(&s.db, &account_id, &folder, uid)?;
                s.mail_cache.remove(&StoreData::cache_key(&account_id, &folder, uid));
            } else {
                // 软删除：归入本地「已删除」虚拟目录
                let mut s = state.inner.lock().unwrap();
                db::set_folder(&s.db, &account_id, &folder, uid, POP3_TRASH)?;
                s.mail_cache.remove(&StoreData::cache_key(&account_id, &folder, uid));
            }
        }
    }
    Ok(())
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
    let (account, cached) = {
        let s = state.inner.lock().unwrap();
        (
            s.account(&account_id)?,
            db::get_raw(&s.db, &account_id, &folder, uid)?.map(|r| r.raw),
        )
    };
    let raw = match cached {
        Some(raw) => raw,
        None => {
            let secret = fresh_secret(&state, &account_id).await?;
            let f2 = folder.clone();
            tauri::async_runtime::spawn_blocking(move || match account.protocol {
                IncomingProtocol::Imap => imap_client::fetch_raw(&account, &secret, &f2, uid),
                IncomingProtocol::Pop3 => Err("邮件不在本地缓存中，请刷新列表后重试".to_string()),
            })
            .await
            .map_err(|e| e.to_string())??
        }
    };
    let (_, contents) = mail::extract_attachment(&raw, index)?;
    std::fs::write(&path, contents).map_err(|e| format!("写入文件失败: {}", e))
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
    let rcpts: Vec<String> = to.iter().chain(cc.iter()).cloned().collect();
    let result = tauri::async_runtime::spawn_blocking(move || {
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
        smtp_client::send_mail(&account, &secret, signer, to, cc, &subject, &body, files)
    })
    .await
    .map_err(|e| e.to_string())?;
    // 发送成功：把收件人收进联系人（自动补全）
    if result.is_ok() {
        let now = chrono::Local::now().timestamp();
        let mut s = state.inner.lock().unwrap();
        for addr in rcpts {
            s.upsert_contact("", &addr, now);
        }
        if let Err(e) = s.save_contacts() {
            eprintln!("[contacts] 保存失败: {}", e);
        }
    }
    result
}

/// 联系人查询（写信自动补全）：按往来次数+最近往来排序，最多 8 条
#[tauri::command]
fn list_contacts(state: State<'_, AppState>, query: Option<String>) -> Vec<Contact> {
    let s = state.inner.lock().unwrap();
    let q = query.unwrap_or_default().trim().to_lowercase();
    let mut list: Vec<&Contact> = s
        .contacts
        .values()
        .filter(|c| q.is_empty() || c.email.to_lowercase().contains(&q) || c.name.to_lowercase().contains(&q))
        .collect();
    list.sort_by(|a, b| b.count.cmp(&a.count).then(b.last_seen.cmp(&a.last_seen)));
    list.into_iter().take(8).cloned().collect()
}

// ───────────────────────── drafts ─────────────────────────

#[tauri::command]
fn list_drafts(state: State<'_, AppState>) -> Vec<Draft> {
    let mut list = state.inner.lock().unwrap().drafts.clone();
    list.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    list
}

#[tauri::command]
fn save_draft(state: State<'_, AppState>, mut draft: Draft) -> Result<Draft, String> {
    if draft.id.is_empty() {
        draft.id = gen_id();
    }
    draft.updated_at = chrono::Local::now().timestamp();
    let mut s = state.inner.lock().unwrap();
    s.drafts.retain(|d| d.id != draft.id);
    s.drafts.push(draft.clone());
    s.save_drafts()?;
    Ok(draft)
}

#[tauri::command]
fn delete_draft(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut s = state.inner.lock().unwrap();
    s.drafts.retain(|d| d.id != id);
    s.save_drafts()
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
    // 先增量同步收件箱，再对本地缓存的最近邮件跑规则
    sync_messages(state.clone(), account_id.clone(), "INBOX".into()).await?;
    let (rules, mails) = {
        let s = state.inner.lock().unwrap();
        let account = s.account(&account_id)?;
        let trusted = s.trusted_for_verify(&account);
        let mails: Vec<EmailFull> = db::list(&s.db, &account_id, "INBOX", 0, 200)?
            .iter()
            .filter_map(|r| {
                mail::parse_email(&r.raw, r.uid, &account_id, "INBOX", r.unread, r.flagged, &trusted).ok()
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
            let dir = app
                .path()
                .app_config_dir()
                .expect("无法获取应用配置目录");
            let data = StoreData::load(dir).expect("初始化本地存储失败");
            app.manage(AppState { inner: std::sync::Mutex::new(data) });
            // 启动新邮件监听（IMAP IDLE / POP3 轮询）
            watcher::ensure_watchers(&app.handle().clone());
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
            list_cached,
            sync_messages,
            get_message,
            move_message,
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
