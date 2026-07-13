use crate::models::*;
use crate::store::StoreData;
use crate::{db, filters, imap_client, mail, pop3_client, smtp_client};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStateView {
    pub accounts: Vec<Account>,
    pub identity: IdentityInfo,
    pub trusted: Vec<TrustedContact>,
    pub filters: Vec<FilterRule>,
    pub local_folders: Vec<String>,
}

pub const POP3_TRASH: &str = "已删除";
pub const POP3_ARCHIVE: &str = "归档";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedList {
    pub metas: Vec<EmailMeta>,
    pub total: i64,
    pub unread_count: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub added: u32,
    pub total: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentSaveResult {
    pub uid: u32,
    pub index: usize,
    pub filename: String,
    pub path: String,
    pub bytes: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentDataResult {
    pub uid: u32,
    pub index: usize,
    pub filename: String,
    pub mime: String,
    pub bytes: usize,
    pub data_base64: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub moved: u32,
    pub details: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailLocation {
    pub folder: String,
    pub uid: u32,
}

pub struct Core {
    pub data: StoreData,
}

impl Core {
    pub fn load(dir: PathBuf) -> Result<Self, String> {
        StoreData::load(dir).map(|data| Core { data })
    }

    pub fn state_view(&self) -> AppStateView {
        state_view(&self.data)
    }

    pub fn identity_info(&self) -> IdentityInfo {
        identity_info(&self.data)
    }

    pub fn contacts(&self, query: Option<String>) -> Vec<Contact> {
        list_contacts(&self.data, query)
    }

    pub fn drafts(&self) -> Vec<Draft> {
        list_drafts(&self.data)
    }
}

fn now_date() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn gen_id() -> String {
    let mut b = [0u8; 8];
    let _ = getrandom::getrandom(&mut b);
    hex::encode(b)
}

pub fn state_view(s: &StoreData) -> AppStateView {
    AppStateView {
        accounts: s.accounts.clone(),
        identity: identity_info(s),
        trusted: s.trusted.clone(),
        filters: s.filters.clone(),
        local_folders: s.local_folders.clone(),
    }
}

pub fn identity_info(s: &StoreData) -> IdentityInfo {
    IdentityInfo {
        fingerprint: s.active_fingerprint(),
        public_key: s.identity.public_key_b64(),
        created: s.identity.created.clone(),
        mode: s.identity_config.mode.clone(),
        ledger_path: s.identity_config.ledger_path.clone(),
        ledger_address: s.identity_config.ledger_address.clone(),
    }
}

pub fn bind_ledger(
    s: &mut StoreData,
    path: String,
    address: String,
) -> Result<IdentityInfo, String> {
    s.identity_config = IdentityConfig {
        mode: "ledger".into(),
        ledger_path: Some(path),
        ledger_address: Some(address.to_lowercase()),
    };
    s.save_identity_config()?;
    Ok(identity_info(s))
}

pub fn use_local_key(s: &mut StoreData) -> Result<IdentityInfo, String> {
    s.identity_config = IdentityConfig::default();
    s.save_identity_config()?;
    Ok(identity_info(s))
}

pub fn get_close_behavior(s: &StoreData) -> String {
    s.prefs.close_behavior.clone()
}

pub fn set_close_behavior(s: &mut StoreData, behavior: String) -> Result<String, String> {
    if behavior != "hide" && behavior != "quit" {
        return Err(format!("无效的关闭行为: {}", behavior));
    }
    s.prefs.close_behavior = behavior.clone();
    s.save_prefs()?;
    Ok(behavior)
}

pub fn get_notify_new_mail(s: &StoreData) -> bool {
    s.prefs.notify_new_mail
}

pub fn set_language(s: &mut StoreData, language: String) -> Result<String, String> {
    if language != "system" && language != "zh" && language != "en" {
        return Err(format!("{}: {}", crate::i18n::tr("无效的语言"), language));
    }
    s.prefs.language = language.clone();
    s.save_prefs()?;
    crate::i18n::set_lang_from_pref(&language);
    Ok(language)
}

pub fn set_notify_new_mail(s: &mut StoreData, enabled: bool) -> Result<bool, String> {
    s.prefs.notify_new_mail = enabled;
    s.save_prefs()?;
    Ok(enabled)
}

pub fn list_contacts(s: &StoreData, query: Option<String>) -> Vec<Contact> {
    let q = query.unwrap_or_default().trim().to_lowercase();
    let mut list: Vec<&Contact> = s
        .contacts
        .values()
        .filter(|c| {
            q.is_empty()
                || c.email.to_lowercase().contains(&q)
                || c.name.to_lowercase().contains(&q)
        })
        .collect();
    list.sort_by(|a, b| b.count.cmp(&a.count).then(b.last_seen.cmp(&a.last_seen)));
    list.into_iter().take(8).cloned().collect()
}

pub fn list_drafts(s: &StoreData) -> Vec<Draft> {
    let mut list = s.drafts.clone();
    list.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    list
}

pub fn save_draft(s: &mut StoreData, mut draft: Draft) -> Result<Draft, String> {
    if draft.id.is_empty() {
        draft.id = gen_id();
    }
    draft.updated_at = chrono::Local::now().timestamp();
    s.drafts.retain(|d| d.id != draft.id);
    s.drafts.push(draft.clone());
    s.save_drafts()?;
    Ok(draft)
}

pub fn delete_draft(s: &mut StoreData, id: String) -> Result<(), String> {
    s.drafts.retain(|d| d.id != id);
    s.save_drafts()
}

pub fn save_filter(s: &mut StoreData, mut rule: FilterRule) -> Result<Vec<FilterRule>, String> {
    if rule.id.is_empty() {
        // 新规则先查重：「屏蔽发件人」按钮重复点击会提交完全相同的规则，
        // 匹配条件+目标一致时更新现有规则而不是追加一条重复的
        match s.filters.iter().find(|f| {
            f.account_id == rule.account_id
                && f.field == rule.field
                && f.op == rule.op
                && f.value == rule.value
                && f.target_folder == rule.target_folder
        }) {
            Some(existing) => rule.id = existing.id.clone(),
            None => rule.id = gen_id(),
        }
    }
    // 规则按顺序优先匹配：更新已有规则时原位替换，不改变它的顺位
    match s.filters.iter_mut().find(|f| f.id == rule.id) {
        Some(slot) => *slot = rule,
        None => s.filters.push(rule),
    }
    s.save_filters()?;
    Ok(s.filters.clone())
}

pub fn delete_filter(s: &mut StoreData, id: String) -> Result<Vec<FilterRule>, String> {
    s.filters.retain(|f| f.id != id);
    s.save_filters()?;
    Ok(s.filters.clone())
}

pub fn trust_sender(
    s: &mut StoreData,
    name: String,
    email: String,
    fingerprint: String,
    org: Option<String>,
) -> Result<Vec<TrustedContact>, String> {
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
    invalidate_meta_cache(s);
    Ok(s.trusted.clone())
}

pub fn remove_trusted(s: &mut StoreData, email: String) -> Result<Vec<TrustedContact>, String> {
    s.trusted.retain(|t| !t.email.eq_ignore_ascii_case(&email));
    s.save_trusted()?;
    invalidate_meta_cache(s);
    Ok(s.trusted.clone())
}

/// 可信联系人变化后，列表里缓存的 trust 标记需按新规则重算：
/// 清空持久化的元信息缓存与内存缓存，下次读取会用最新信任关系重新解析。
fn invalidate_meta_cache(s: &mut StoreData) {
    let _ = db::clear_all_meta_json(&s.db);
    s.mail_cache.clear();
}

pub fn test_connection(account: &Account, secret: &AccountSecret) -> Result<(), String> {
    match account.protocol {
        IncomingProtocol::Imap => {
            imap_client::connect(account, secret).map(|mut s| {
                let _ = s.logout();
            })?;
        }
        IncomingProtocol::Pop3 => {
            let mut c = pop3_client::Pop3Client::connect(account, secret)?;
            c.quit();
        }
    }
    smtp_client::test_smtp(account, secret)
}

pub fn add_account(
    s: &mut StoreData,
    mut account: Account,
    secret: AccountSecret,
) -> Result<Account, String> {
    if account.id.is_empty() {
        account.id = gen_id();
    }
    test_connection(&account, &secret)?;
    save_account(s, account, secret)
}

pub fn save_account(
    s: &mut StoreData,
    account: Account,
    secret: AccountSecret,
) -> Result<Account, String> {
    s.accounts.retain(|a| a.id != account.id);
    s.accounts.push(account.clone());
    s.secrets.insert(account.id.clone(), secret);
    s.save_accounts()?;
    s.save_secrets()?;
    Ok(account)
}

pub fn remove_account(s: &mut StoreData, account_id: String) -> Result<(), String> {
    s.accounts.retain(|a| a.id != account_id);
    s.secrets.remove(&account_id);
    s.save_accounts()?;
    s.save_secrets()
}

pub fn list_folders(s: &StoreData, account_id: &str) -> Result<Vec<FolderInfo>, String> {
    let account = s.account(account_id)?;
    let secret = s.secret(account_id)?;
    match account.protocol {
        IncomingProtocol::Imap => {
            let hidden = s.hidden_folders.get(account_id).cloned().unwrap_or_default();
            let mut folders = imap_client::list_folders(&account, &secret)?;
            folders.retain(|folder| !hidden.contains(&folder.name));
            Ok(folders)
        }
        IncomingProtocol::Pop3 => {
            let mut out = vec![
                FolderInfo {
                    name: "INBOX".into(),
                    display: "收件箱".into(),
                    role: None,
                },
                FolderInfo {
                    name: "已删除".into(),
                    display: "已删除".into(),
                    role: Some("trash".into()),
                },
            ];
            out.extend(
                s.local_folders
                    .iter()
                    .filter(|f| f.as_str() != "已删除")
                    .map(|folder| FolderInfo {
                        name: folder.clone(),
                        display: folder.clone(),
                        role: None,
                    }),
            );
            Ok(out)
        }
    }
}

pub fn delete_folder(s: &mut StoreData, account_id: &str, folder: &str) -> Result<(), String> {
    if folder == "INBOX" || folder == POP3_TRASH || folder == POP3_ARCHIVE {
        return Err("系统目录不能删除".into());
    }
    let account = s.account(account_id)?;
    let secret = s.secret(account_id)?;
    match account.protocol {
        IncomingProtocol::Imap => {
            match imap_client::delete_folder(&account, &secret, folder) {
                Ok(()) => {}
                Err(_) => {
                    let hidden = s.hidden_folders.entry(account_id.to_string()).or_default();
                    if !hidden.contains(&folder.to_string()) {
                        hidden.push(folder.to_string());
                        s.save_hidden_folders()?;
                    }
                }
            }
            db::clear_folder(&s.db, account_id, folder)
        }
        IncomingProtocol::Pop3 => {
            s.local_folders.retain(|f| f != folder);
            s.save_local_folders()?;
            db::clear_folder(&s.db, account_id, folder)?;
            Ok(())
        }
    }
}

pub fn create_folder(s: &mut StoreData, account_id: &str, name: String) -> Result<(), String> {
    let account = s.account(account_id)?;
    let secret = s.secret(account_id)?;
    match account.protocol {
        IncomingProtocol::Imap => {
            imap_client::create_folder(&account, &secret, &name)?;
            if let Some(hidden) = s.hidden_folders.get_mut(account_id) {
                hidden.retain(|folder| folder != &name);
                s.save_hidden_folders()?;
            }
            Ok(())
        }
        IncomingProtocol::Pop3 => {
            if !s.local_folders.contains(&name) {
                s.local_folders.push(name);
                s.save_local_folders()?;
            }
            Ok(())
        }
    }
}

pub fn list_cached(
    s: &mut StoreData,
    account_id: &str,
    folder: &str,
    offset: u32,
    limit: u32,
) -> Result<CachedList, String> {
    let account = s.account(account_id)?;
    let trusted = s.trusted_for_verify(&account);
    // 只读元信息缓存（不含 raw），命中即秒出；未命中才读 raw 解析一次并写回缓存。
    let rows = db::list_meta(&s.db, account_id, folder, offset, limit.min(200))?;
    let total = db::count(&s.db, account_id, folder)?;
    let unread_count = db::count_unread(&s.db, account_id, folder)?;
    let mut metas = Vec::with_capacity(rows.len());
    for r in rows {
        let cached = r
            .meta_json
            .as_deref()
            .and_then(|j| serde_json::from_str::<EmailMeta>(j).ok());
        let mut meta = match cached {
            Some(m) => m,
            None => {
                // 旧数据/未缓存：读一次完整 raw 解析，并把元信息写回，下次即可秒开。
                let raw = match db::get_raw(&s.db, account_id, folder, r.uid)? {
                    Some(raw) => raw,
                    None => continue,
                };
                let full = match mail::parse_email(
                    &raw.raw, r.uid, account_id, folder, raw.unread, raw.flagged, &trusted,
                ) {
                    Ok(full) => full,
                    Err(e) => {
                        eprintln!("[cache] 解析缓存邮件失败 uid={}: {}", r.uid, e);
                        continue;
                    }
                };
                if let Ok(json) = serde_json::to_string(&full.meta) {
                    let _ = db::set_meta_json(&s.db, account_id, folder, r.uid, &json);
                }
                let meta = full.meta.clone();
                s.mail_cache
                    .insert(StoreData::cache_key(account_id, folder, r.uid), full);
                meta
            }
        };
        // 已读/星标以 DB 当前列为准（标记操作只更新列、不动元信息缓存）。
        meta.unread = r.unread;
        meta.flagged = r.flagged;
        metas.push(meta);
    }
    Ok(CachedList {
        metas,
        total,
        unread_count,
    })
}

pub fn sync_messages(
    s: &mut StoreData,
    account_id: &str,
    folder: &str,
    secret: &AccountSecret,
) -> Result<SyncResult, String> {
    let account = s.account(account_id)?;
    let trusted = s.trusted_for_verify(&account);
    match account.protocol {
        IncomingProtocol::Imap => {
            let validity = db::uidvalidity(&s.db, account_id, folder)?;
            let max_uid = db::max_uid(&s.db, account_id, folder)?;
            let low = db::window_low(&s.db, account_id, folder, db::FLAG_SYNC_WINDOW)?;
            let sf = imap_client::sync_fetch(
                &account,
                secret,
                folder,
                validity,
                max_uid,
                low,
                db::INITIAL_WINDOW,
            )?;
            if sf.reset {
                db::clear_folder(&s.db, account_id, folder)?;
            }
            db::set_uidvalidity(&s.db, account_id, folder, sf.uidvalidity)?;
            let mut added = 0u32;
            let mut new_fulls: Vec<EmailFull> = Vec::new();
            for m in &sf.new_mails {
                let ts = match mail::parse_email(
                    &m.raw, m.uid, account_id, folder, m.unread, m.flagged, &trusted,
                ) {
                    Ok(full) => {
                        s.upsert_contact(
                            &full.meta.from_name,
                            &full.meta.from_addr,
                            full.meta.timestamp,
                        );
                        let ts = full.meta.timestamp;
                        new_fulls.push(full);
                        ts
                    }
                    Err(_) => 0,
                };
                db::upsert_message(
                    &s.db, account_id, folder, m.uid, None, m.unread, m.flagged, ts, &m.raw,
                )?;
                added += 1;
            }
            if !sf.reset {
                let server: std::collections::HashMap<u32, (bool, bool)> = sf
                    .server_flags
                    .iter()
                    .map(|(u, a, b)| (*u, (*a, *b)))
                    .collect();
                for uid in db::uids_from(&s.db, account_id, folder, sf.flags_low)? {
                    match server.get(&uid) {
                        Some((unread, flagged)) => {
                            db::update_flags(&s.db, account_id, folder, uid, *unread, *flagged)?
                        }
                        None => {
                            db::delete_row(&s.db, account_id, folder, uid)?;
                            s.mail_cache
                                .remove(&StoreData::cache_key(account_id, folder, uid));
                        }
                    }
                }
            }
            if let Err(e) = s.save_contacts() {
                eprintln!("[contacts] 保存失败: {}", e);
            }
            // 新到收件箱的邮件自动应用过滤规则（单连接批量，见 organize_by_filters）
            if folder == "INBOX" && !new_fulls.is_empty() {
                organize_by_filters(s, account_id, secret, "INBOX", &new_fulls)?;
            }
            Ok(SyncResult {
                added,
                total: db::count(&s.db, account_id, folder)?,
            })
        }
        IncomingProtocol::Pop3 => {
            let known: std::collections::HashSet<String> = db::pop_known_uidls(&s.db, account_id)?
                .into_iter()
                .map(|(uidl, _, _)| uidl)
                .collect();
            let ps = pop3_client::sync_fetch(&account, secret, &known, db::INITIAL_WINDOW)?;
            let mut added = 0u32;
            let mut new_fulls: Vec<EmailFull> = Vec::new();
            for (uidl, raw) in &ps.new_mails {
                let uid = db::pop_next_uid(&s.db, account_id)?;
                let ts =
                    match mail::parse_email(raw, uid, account_id, "INBOX", true, false, &trusted) {
                        Ok(full) => {
                            s.upsert_contact(
                                &full.meta.from_name,
                                &full.meta.from_addr,
                                full.meta.timestamp,
                            );
                            let ts = full.meta.timestamp;
                            new_fulls.push(full);
                            ts
                        }
                        Err(_) => 0,
                    };
                db::upsert_message(
                    &s.db,
                    account_id,
                    "INBOX",
                    uid,
                    Some(uidl),
                    true,
                    false,
                    ts,
                    raw,
                )?;
                added += 1;
            }
            let server: std::collections::HashSet<&String> = ps.all_uidls.iter().collect();
            for (uidl, fld, uid) in db::pop_known_uidls(&s.db, account_id)? {
                if !server.contains(&uidl) {
                    db::delete_row(&s.db, account_id, &fld, uid)?;
                    s.mail_cache
                        .remove(&StoreData::cache_key(account_id, &fld, uid));
                }
            }
            if let Err(e) = s.save_contacts() {
                eprintln!("[contacts] 保存失败: {}", e);
            }
            // 新到收件箱的邮件自动应用过滤规则（POP3 为本地虚拟目录，纯本地操作）
            if !new_fulls.is_empty() {
                organize_by_filters(s, account_id, secret, "INBOX", &new_fulls)?;
            }
            Ok(SyncResult {
                added,
                total: db::count(&s.db, account_id, folder)?,
            })
        }
    }
}

pub fn sync_older_messages(
    s: &mut StoreData,
    account_id: &str,
    folder: &str,
    secret: &AccountSecret,
) -> Result<SyncResult, String> {
    let account = s.account(account_id)?;
    let trusted = s.trusted_for_verify(&account);
    match account.protocol {
        IncomingProtocol::Imap => {
            let before_uid = db::min_uid(&s.db, account_id, folder)?;
            let mails =
                imap_client::fetch_older(&account, secret, folder, before_uid, db::OLDER_WINDOW)?;
            let mut added = 0u32;
            for m in &mails {
                let ts = match mail::parse_email(
                    &m.raw, m.uid, account_id, folder, m.unread, m.flagged, &trusted,
                ) {
                    Ok(full) => {
                        s.upsert_contact(
                            &full.meta.from_name,
                            &full.meta.from_addr,
                            full.meta.timestamp,
                        );
                        full.meta.timestamp
                    }
                    Err(_) => 0,
                };
                db::upsert_message(
                    &s.db, account_id, folder, m.uid, None, m.unread, m.flagged, ts, &m.raw,
                )?;
                added += 1;
            }
            if let Err(e) = s.save_contacts() {
                eprintln!("[contacts] 保存失败: {}", e);
            }
            Ok(SyncResult {
                added,
                total: db::count(&s.db, account_id, folder)?,
            })
        }
        IncomingProtocol::Pop3 => {
            let known: std::collections::HashSet<String> = db::pop_known_uidls(&s.db, account_id)?
                .into_iter()
                .map(|(uidl, _, _)| uidl)
                .collect();
            let ps = pop3_client::fetch_unknown_window(&account, secret, &known, db::OLDER_WINDOW)?;
            let mut added = 0u32;
            for (uidl, raw) in &ps.new_mails {
                let uid = db::pop_next_uid(&s.db, account_id)?;
                let ts =
                    match mail::parse_email(raw, uid, account_id, "INBOX", true, false, &trusted) {
                        Ok(full) => {
                            s.upsert_contact(
                                &full.meta.from_name,
                                &full.meta.from_addr,
                                full.meta.timestamp,
                            );
                            full.meta.timestamp
                        }
                        Err(_) => 0,
                    };
                db::upsert_message(
                    &s.db,
                    account_id,
                    "INBOX",
                    uid,
                    Some(uidl),
                    true,
                    false,
                    ts,
                    raw,
                )?;
                added += 1;
            }
            Ok(SyncResult {
                added,
                total: db::count(&s.db, account_id, folder)?,
            })
        }
    }
}

pub fn get_message(
    s: &mut StoreData,
    account_id: &str,
    folder: &str,
    uid: u32,
) -> Result<EmailFull, String> {
    let key = StoreData::cache_key(account_id, folder, uid);
    if let Some(full) = s.mail_cache.get(&key) {
        return Ok(full.clone());
    }
    let account = s.account(account_id)?;
    let trusted = s.trusted_for_verify(&account);
    let row =
        db::get_raw(&s.db, account_id, folder, uid)?.ok_or("邮件不在本地缓存中，请刷新列表")?;
    let full = mail::parse_email(
        &row.raw,
        uid,
        account_id,
        folder,
        row.unread,
        row.flagged,
        &trusted,
    )?;
    s.mail_cache.insert(key, full.clone());
    Ok(full)
}

/// 后台补全一批 meta_json 缓存（升级后首次运行的一次性开销，从交互路径挪到后台）。
/// 从 `after_uid` 之后按 uid 升序处理至多 `limit` 行；返回 (处理行数, 本批最大 uid)。
/// 解析失败的行也推进游标（否则会原地打转），下次列表读到时再按需处理。
pub fn backfill_meta_batch(
    s: &mut StoreData,
    account_id: &str,
    folder: &str,
    after_uid: u32,
    limit: u32,
) -> Result<(u32, Option<u32>), String> {
    let account = s.account(account_id)?; // 账户已删除时报错，调用方跳过该目录
    let trusted = s.trusted_for_verify(&account);
    let rows = db::rows_missing_meta(&s.db, account_id, folder, after_uid, limit)?;
    let n = rows.len() as u32;
    let max_uid = rows.last().map(|r| r.uid);
    for r in rows {
        if let Ok(full) =
            mail::parse_email(&r.raw, r.uid, account_id, folder, r.unread, r.flagged, &trusted)
        {
            if let Ok(json) = serde_json::to_string(&full.meta) {
                let _ = db::set_meta_json(&s.db, account_id, folder, r.uid, &json);
            }
        }
    }
    Ok((n, max_uid))
}

pub fn list_thread(
    s: &mut StoreData,
    account_id: &str,
    folder: &str,
    thread_id: &str,
) -> Result<Vec<EmailMeta>, String> {
    let account = s.account(account_id)?;
    let trusted = s.trusted_for_verify(&account);
    let mut metas = Vec::new();
    // 走元信息缓存（不含 raw）：命中即用，未命中才读 raw 解析一次并写回。
    for r in db::list_folder_meta(&s.db, account_id, folder)? {
        let cached = r
            .meta_json
            .as_deref()
            .and_then(|j| serde_json::from_str::<EmailMeta>(j).ok());
        let mut meta = match cached {
            Some(m) => m,
            None => {
                let raw = match db::get_raw(&s.db, account_id, folder, r.uid)? {
                    Some(raw) => raw,
                    None => continue,
                };
                let full = match mail::parse_email(
                    &raw.raw, r.uid, account_id, folder, raw.unread, raw.flagged, &trusted,
                ) {
                    Ok(full) => full,
                    Err(e) => {
                        eprintln!("[thread] 解析缓存邮件失败 uid={}: {}", r.uid, e);
                        continue;
                    }
                };
                if let Ok(json) = serde_json::to_string(&full.meta) {
                    let _ = db::set_meta_json(&s.db, account_id, folder, r.uid, &json);
                }
                let meta = full.meta.clone();
                s.mail_cache
                    .insert(StoreData::cache_key(account_id, folder, r.uid), full);
                meta
            }
        };
        meta.unread = r.unread;
        meta.flagged = r.flagged;
        if meta.thread_id == thread_id {
            metas.push(meta);
        }
    }
    metas.sort_by_key(|m| m.timestamp);
    Ok(metas)
}

pub fn move_message(
    s: &mut StoreData,
    account_id: &str,
    folder: &str,
    uid: u32,
    target: &str,
    secret: &AccountSecret,
) -> Result<(), String> {
    let account = s.account(account_id)?;
    match account.protocol {
        IncomingProtocol::Imap => {
            imap_client::move_message(&account, secret, folder, uid, target)?;
            db::delete_row(&s.db, account_id, folder, uid)?;
            s.mail_cache
                .remove(&StoreData::cache_key(account_id, folder, uid));
        }
        IncomingProtocol::Pop3 => {
            db::set_folder(&s.db, account_id, folder, uid, target)?;
            if let Some(mut full) = s
                .mail_cache
                .remove(&StoreData::cache_key(account_id, folder, uid))
            {
                full.meta.folder = target.to_string();
                s.mail_cache
                    .insert(StoreData::cache_key(account_id, target, uid), full);
            }
        }
    }
    Ok(())
}

pub fn archive_message(
    s: &mut StoreData,
    account_id: &str,
    folder: &str,
    uid: u32,
    secret: &AccountSecret,
) -> Result<(), String> {
    let account = s.account(account_id)?;
    match account.protocol {
        IncomingProtocol::Imap => {
            let target = imap_client::archive_message(&account, secret, folder, uid)?;
            if !target.eq_ignore_ascii_case(folder) {
                db::delete_row(&s.db, account_id, folder, uid)?;
                s.mail_cache
                    .remove(&StoreData::cache_key(account_id, folder, uid));
            }
        }
        IncomingProtocol::Pop3 => {
            if !s.local_folders.contains(&POP3_ARCHIVE.to_string()) {
                s.local_folders.push(POP3_ARCHIVE.into());
                s.save_local_folders()?;
            }
            db::set_folder(&s.db, account_id, folder, uid, POP3_ARCHIVE)?;
            if let Some(mut full) = s
                .mail_cache
                .remove(&StoreData::cache_key(account_id, folder, uid))
            {
                full.meta.folder = POP3_ARCHIVE.into();
                s.mail_cache
                    .insert(StoreData::cache_key(account_id, POP3_ARCHIVE, uid), full);
            }
        }
    }
    Ok(())
}

pub fn set_read(
    s: &mut StoreData,
    account_id: &str,
    folder: &str,
    uids: &[u32],
    read: bool,
    secret: &AccountSecret,
) -> Result<(), String> {
    let account = s.account(account_id)?;
    if account.protocol == IncomingProtocol::Imap {
        imap_client::set_read_many(&account, secret, folder, uids, read)?;
    }
    db::set_unread(&s.db, account_id, folder, uids, !read)?;
    for uid in uids {
        if let Some(full) = s
            .mail_cache
            .get_mut(&StoreData::cache_key(account_id, folder, *uid))
        {
            full.meta.unread = !read;
        }
    }
    Ok(())
}

pub fn set_flagged(
    s: &mut StoreData,
    account_id: &str,
    folder: &str,
    uid: u32,
    flagged: bool,
    secret: &AccountSecret,
) -> Result<(), String> {
    let account = s.account(account_id)?;
    if account.protocol == IncomingProtocol::Imap {
        imap_client::set_flagged(&account, secret, folder, uid, flagged)?;
    }
    db::set_flagged(&s.db, account_id, folder, uid, flagged)?;
    if let Some(full) = s
        .mail_cache
        .get_mut(&StoreData::cache_key(account_id, folder, uid))
    {
        full.meta.flagged = flagged;
    }
    Ok(())
}

pub fn delete_message(
    s: &mut StoreData,
    account_id: &str,
    folder: &str,
    uid: u32,
    permanent: bool,
    secret: &AccountSecret,
) -> Result<(), String> {
    let account = s.account(account_id)?;
    match account.protocol {
        IncomingProtocol::Imap => {
            imap_client::delete_message(&account, secret, folder, uid, permanent)?;
            db::delete_row(&s.db, account_id, folder, uid)?;
            s.mail_cache
                .remove(&StoreData::cache_key(account_id, folder, uid));
        }
        IncomingProtocol::Pop3 => {
            if permanent {
                let uidl = db::pop_uidl_of(&s.db, account_id, folder, uid)?
                    .ok_or("本地缓存缺少该邮件的服务器标识，请刷新后重试")?;
                pop3_client::delete_by_uidl(&account, secret, &uidl)?;
                db::delete_row(&s.db, account_id, folder, uid)?;
            } else {
                db::set_folder(&s.db, account_id, folder, uid, POP3_TRASH)?;
            }
            s.mail_cache
                .remove(&StoreData::cache_key(account_id, folder, uid));
        }
    }
    Ok(())
}

fn load_raw_message(
    s: &StoreData,
    account_id: &str,
    folder: &str,
    uid: u32,
    secret: Option<&AccountSecret>,
) -> Result<Vec<u8>, String> {
    let account = s.account(account_id)?;
    let cached = db::get_raw(&s.db, account_id, folder, uid)?.map(|r| r.raw);
    match cached {
        Some(raw) => Ok(raw),
        None => match account.protocol {
            IncomingProtocol::Imap => {
                let secret = secret.ok_or("邮件不在本地缓存中，回源下载附件需要账户凭据")?;
                imap_client::fetch_raw(&account, secret, folder, uid)
            }
            IncomingProtocol::Pop3 => Err("邮件不在本地缓存中，请刷新列表后重试".into()),
        },
    }
}

pub fn save_attachment(
    s: &StoreData,
    account_id: &str,
    folder: &str,
    uid: u32,
    index: usize,
    path: &str,
    secret: Option<&AccountSecret>,
) -> Result<AttachmentSaveResult, String> {
    let raw = load_raw_message(s, account_id, folder, uid, secret)?;
    let extracted = mail::extract_attachment(&raw, index)?;
    let bytes = extracted.contents.len();
    let filename = extracted.filename.clone();
    std::fs::write(path, extracted.contents).map_err(|e| format!("写入文件失败: {}", e))?;
    Ok(AttachmentSaveResult {
        uid,
        index,
        filename,
        path: path.to_string(),
        bytes,
    })
}

/// 读取附件内容（base64），供前端图片预览等场景使用
pub fn read_attachment(
    s: &StoreData,
    account_id: &str,
    folder: &str,
    uid: u32,
    index: usize,
    secret: Option<&AccountSecret>,
) -> Result<AttachmentDataResult, String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let raw = load_raw_message(s, account_id, folder, uid, secret)?;
    let extracted = mail::extract_attachment(&raw, index)?;
    let bytes = extracted.contents.len();
    Ok(AttachmentDataResult {
        uid,
        index,
        filename: extracted.filename,
        mime: extracted.mime,
        bytes,
        data_base64: STANDARD.encode(extracted.contents),
    })
}

pub fn send_mail(
    s: &mut StoreData,
    account_id: &str,
    secret: &AccountSecret,
    to: Vec<String>,
    cc: Vec<String>,
    subject: &str,
    body: &str,
    sign: bool,
    attachments: Vec<(String, Vec<u8>)>,
) -> Result<smtp_client::SendResult, String> {
    let account = s.account(account_id)?;
    let signer = if sign {
        if s.identity_config.mode == "ledger" {
            let path = s
                .identity_config
                .ledger_path
                .clone()
                .ok_or("Ledger 未绑定派生路径，请在「身份与密钥」重新绑定")?;
            let address = s
                .identity_config
                .ledger_address
                .clone()
                .ok_or("Ledger 未绑定地址，请在「身份与密钥」重新绑定")?;
            smtp_client::Signer::Ledger { path, address }
        } else {
            smtp_client::Signer::Local(&s.identity)
        }
    } else {
        smtp_client::Signer::None
    };
    let rcpts: Vec<String> = to.iter().chain(cc.iter()).cloned().collect();
    let result =
        smtp_client::send_mail(&account, secret, signer, to, cc, subject, body, attachments)?;
    let now = chrono::Local::now().timestamp();
    for addr in rcpts {
        s.upsert_contact("", &addr, now);
    }
    if let Err(e) = s.save_contacts() {
        eprintln!("[contacts] 保存失败: {}", e);
    }
    Ok(result)
}

pub fn apply_filters(
    s: &mut StoreData,
    account_id: &str,
    secret: &AccountSecret,
) -> Result<ApplyResult, String> {
    sync_messages(s, account_id, "INBOX", secret)?;
    let account = s.account(account_id)?;
    let trusted = s.trusted_for_verify(&account);
    let mails: Vec<EmailFull> = db::list(&s.db, account_id, "INBOX", 0, 200)?
        .iter()
        .filter_map(|r| {
            mail::parse_email(
                &r.raw, r.uid, account_id, "INBOX", r.unread, r.flagged, &trusted,
            )
            .ok()
        })
        .collect();
    organize_by_filters(s, account_id, secret, "INBOX", &mails)
}

/// 按过滤规则批量整理给定邮件。IMAP 走单条连接完成全部标已读 + 移动
/// （逐封各开一条 TLS 连接太慢，且移动后旧 UID 失效不能再改标记）。
pub fn organize_by_filters(
    s: &mut StoreData,
    account_id: &str,
    secret: &AccountSecret,
    folder: &str,
    mails: &[EmailFull],
) -> Result<ApplyResult, String> {
    let plans = filters::plan_moves(&s.filters, account_id, folder, mails);
    if plans.is_empty() {
        return Ok(ApplyResult {
            moved: 0,
            details: Vec::new(),
        });
    }
    let account = s.account(account_id)?;
    match account.protocol {
        IncomingProtocol::Imap => {
            let groups: Vec<(String, Vec<u32>, bool)> = plans
                .iter()
                .map(|p| (p.target.clone(), p.uids.clone(), p.mark_read))
                .collect();
            imap_client::organize_messages(&account, secret, folder, &groups)?;
            for plan in &plans {
                for uid in &plan.uids {
                    db::delete_row(&s.db, account_id, folder, *uid)?;
                    s.mail_cache
                        .remove(&StoreData::cache_key(account_id, folder, *uid));
                }
            }
        }
        IncomingProtocol::Pop3 => {
            for plan in &plans {
                for uid in &plan.uids {
                    db::set_folder(&s.db, account_id, folder, *uid, &plan.target)?;
                    if plan.mark_read {
                        db::set_unread(&s.db, account_id, &plan.target, &[*uid], false)?;
                    }
                    if let Some(mut full) = s
                        .mail_cache
                        .remove(&StoreData::cache_key(account_id, folder, *uid))
                    {
                        full.meta.folder = plan.target.clone();
                        if plan.mark_read {
                            full.meta.unread = false;
                        }
                        s.mail_cache
                            .insert(StoreData::cache_key(account_id, &plan.target, *uid), full);
                    }
                }
            }
        }
    }
    let mut moved = 0u32;
    let mut details = Vec::new();
    for plan in &plans {
        // 目录名是 IMAP Modified UTF-7 编码，展示给用户前要解码（如 &ZzpWaE66- → 机器人）
        let target_display = imap_client::decode_mutf7(&plan.target);
        for subject in &plan.subjects {
            details.push(format!("「{}」→ {}", subject, target_display));
            moved += 1;
        }
    }
    Ok(ApplyResult { moved, details })
}

/// 在本地缓存里按 Message-ID 定位邮件当前所在的目录和 UID。
/// 候选行还要解析头部核对——回复邮件的 References 里也含同一个 Message-ID。
pub fn locate_in_db(
    s: &mut StoreData,
    account_id: &str,
    message_id: &str,
) -> Result<Option<MailLocation>, String> {
    let candidates = db::find_candidates_containing(&s.db, account_id, message_id, 20)?;
    for (folder, uid, raw) in candidates {
        let Ok(full) = mail::parse_email(&raw, uid, account_id, &folder, false, false, &[]) else {
            continue;
        };
        if full.meta.message_id.as_deref() == Some(message_id) {
            return Ok(Some(MailLocation { folder, uid }));
        }
    }
    Ok(None)
}

/// 点通知跳转的兜底定位：邮件可能已被过滤规则移出 INBOX。
/// 先查本地缓存；未命中时把启用规则的目标目录同步一遍（移动刚发生、目标目录
/// 本地还没同步到的情况）再查一次。
pub fn locate_message(
    s: &mut StoreData,
    account_id: &str,
    secret: &AccountSecret,
    message_id: &str,
) -> Result<Option<MailLocation>, String> {
    if let Some(loc) = locate_in_db(s, account_id, message_id)? {
        return Ok(Some(loc));
    }
    let mut targets: Vec<String> = s
        .filters
        .iter()
        .filter(|r| {
            r.enabled
                && r.account_id
                    .as_ref()
                    .map(|id| id == account_id)
                    .unwrap_or(true)
        })
        .map(|r| r.target_folder.clone())
        .collect();
    targets.sort();
    targets.dedup();
    if targets.is_empty() {
        return Ok(None);
    }
    for folder in &targets {
        if let Err(e) = sync_messages(s, account_id, folder, secret) {
            crate::logging::log(format!("[locate] 同步规则目标目录 {folder} 失败: {e}"));
        }
    }
    locate_in_db(s, account_id, message_id)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyFlowReport {
    pub account_id: String,
    pub subject: String,
    pub test_folder: String,
    pub steps: Vec<String>,
    pub message_uid: u32,
    pub final_folder: String,
}

fn push_step(steps: &mut Vec<String>, step: impl Into<String>) {
    steps.push(step.into());
}

fn sync_imap_folder(
    s: &mut StoreData,
    account: &Account,
    secret: &AccountSecret,
    folder: &str,
) -> Result<u32, String> {
    let validity = db::uidvalidity(&s.db, &account.id, folder)?;
    let max_uid = db::max_uid(&s.db, &account.id, folder)?;
    let low = db::window_low(&s.db, &account.id, folder, db::FLAG_SYNC_WINDOW)?;
    let fetched = imap_client::sync_fetch(
        account,
        secret,
        folder,
        validity,
        max_uid,
        low,
        db::INITIAL_WINDOW,
    )?;
    if fetched.reset {
        db::clear_folder(&s.db, &account.id, folder)?;
    }
    db::set_uidvalidity(&s.db, &account.id, folder, fetched.uidvalidity)?;
    for m in &fetched.new_mails {
        let timestamp = mail::parse_email(
            &m.raw,
            m.uid,
            &account.id,
            folder,
            m.unread,
            m.flagged,
            &s.trusted_for_verify(account),
        )
        .map(|full| full.meta.timestamp)
        .unwrap_or(0);
        db::upsert_message(
            &s.db,
            &account.id,
            folder,
            m.uid,
            None,
            m.unread,
            m.flagged,
            timestamp,
            &m.raw,
        )?;
    }
    for (uid, unread, flagged) in fetched.server_flags {
        db::update_flags(&s.db, &account.id, folder, uid, unread, flagged)?;
    }
    Ok(fetched.new_mails.len() as u32)
}

fn find_subject_uid(
    s: &StoreData,
    account: &Account,
    folder: &str,
    subject: &str,
) -> Result<Option<u32>, String> {
    let trusted = s.trusted_for_verify(account);
    let rows = db::list(&s.db, &account.id, folder, 0, 50)?;
    for row in rows {
        let full = mail::parse_email(
            &row.raw,
            row.uid,
            &account.id,
            folder,
            row.unread,
            row.flagged,
            &trusted,
        )?;
        if full.meta.subject == subject {
            return Ok(Some(row.uid));
        }
    }
    Ok(None)
}

pub fn run_imap_daily_flow(s: &mut StoreData, account_id: &str) -> Result<DailyFlowReport, String> {
    let account = s.account(account_id)?;
    if account.protocol != IncomingProtocol::Imap {
        return Err("真实 daily-flow 目前只支持 IMAP 账户".into());
    }
    let secret = s.secret(account_id)?;
    let stamp = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
    let subject = format!("SealMail E2E {stamp}");
    let test_folder = format!("SealMail-E2E-{stamp}");
    let mut steps = Vec::new();

    test_connection(&account, &secret)?;
    push_step(&mut steps, "连接测试通过：IMAP 登录 + SMTP 连接");

    let folders = imap_client::list_folders(&account, &secret)?;
    if !folders.iter().any(|f| f.name.eq_ignore_ascii_case("INBOX")) {
        return Err("IMAP 目录列表缺少 INBOX".into());
    }
    push_step(
        &mut steps,
        format!("目录列表通过：{} 个目录", folders.len()),
    );

    imap_client::create_folder(&account, &secret, &test_folder)?;
    push_step(&mut steps, format!("创建测试目录：{test_folder}"));

    let body =
        format!("SealMail real mailbox daily-flow test.\nSubject: {subject}\nCreated: {stamp}\n");
    let send_result = smtp_client::send_mail(
        &account,
        &secret,
        smtp_client::Signer::Local(&s.identity),
        vec![account.email.clone()],
        Vec::new(),
        &subject,
        &body,
        Vec::new(),
    )?;
    if !send_result.signed {
        return Err("测试邮件应该带 SealMail 签名".into());
    }
    push_step(&mut steps, "发送自签名测试邮件通过");

    let mut uid = None;
    for _ in 0..12 {
        sync_imap_folder(s, &account, &secret, "INBOX")?;
        uid = find_subject_uid(s, &account, "INBOX", &subject)?;
        if uid.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
    let uid = uid.ok_or_else(|| format!("发送后未能在 INBOX 同步到测试邮件：{subject}"))?;
    push_step(&mut steps, format!("同步收件箱通过：找到 UID {uid}"));

    let raw = imap_client::fetch_raw(&account, &secret, "INBOX", uid)?;
    let full = mail::parse_email(
        &raw,
        uid,
        &account.id,
        "INBOX",
        true,
        false,
        &s.trusted_for_verify(&account),
    )?;
    if full.meta.subject != subject {
        return Err("读取邮件详情 subject 不匹配".into());
    }
    if full.verify.trust_tag() != "verified" {
        return Err(format!(
            "自签名测试邮件验证状态异常：{}",
            full.verify.trust_tag()
        ));
    }
    push_step(&mut steps, "读取详情与自签名验证通过");

    imap_client::set_read(&account, &secret, "INBOX", uid, true)?;
    imap_client::set_read(&account, &secret, "INBOX", uid, false)?;
    push_step(&mut steps, "已读/未读切换通过");

    imap_client::set_flagged(&account, &secret, "INBOX", uid, true)?;
    imap_client::set_flagged(&account, &secret, "INBOX", uid, false)?;
    push_step(&mut steps, "星标/取消星标通过");

    imap_client::move_message(&account, &secret, "INBOX", uid, &test_folder)?;
    push_step(&mut steps, "移动到测试目录通过");

    sync_imap_folder(s, &account, &secret, &test_folder)?;
    let moved_uid = find_subject_uid(s, &account, &test_folder, &subject)?
        .ok_or_else(|| "移动后未能在测试目录同步到邮件".to_string())?;
    let moved_raw = imap_client::fetch_raw(&account, &secret, &test_folder, moved_uid)?;
    let moved = mail::parse_email(
        &moved_raw,
        moved_uid,
        &account.id,
        &test_folder,
        true,
        false,
        &s.trusted_for_verify(&account),
    )?;
    if moved.meta.subject != subject {
        return Err("测试目录读取邮件 subject 不匹配".into());
    }
    push_step(&mut steps, format!("测试目录读取通过：UID {moved_uid}"));

    imap_client::delete_message(&account, &secret, &test_folder, moved_uid, false)?;
    push_step(&mut steps, "软删除到回收站通过");

    match imap_client::delete_folder(&account, &secret, &test_folder) {
        Ok(()) => push_step(&mut steps, "删除测试目录通过"),
        Err(e) => push_step(&mut steps, format!("删除测试目录失败，可能需手动清理：{e}")),
    }

    Ok(DailyFlowReport {
        account_id: account.id,
        subject,
        test_folder,
        steps,
        message_uid: moved_uid,
        final_folder: "trash".into(),
    })
}

pub fn default_config_dir() -> Result<PathBuf, String> {
    if let Ok(dir) = std::env::var("SEALMAIL_CONFIG_DIR") {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }

    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").map_err(|_| "无法定位 HOME 目录".to_string())?;
        return Ok(PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("com.sealmail.app"));
    }

    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").map_err(|_| "无法定位 APPDATA 目录".to_string())?;
        return Ok(PathBuf::from(appdata).join("com.sealmail.app"));
    }

    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            let trimmed = xdg.trim();
            if !trimmed.is_empty() {
                return Ok(PathBuf::from(trimmed).join("com.sealmail.app"));
            }
        }
        let home = std::env::var("HOME").map_err(|_| "无法定位 HOME 目录".to_string())?;
        Ok(PathBuf::from(home).join(".config").join("com.sealmail.app"))
    }
}
