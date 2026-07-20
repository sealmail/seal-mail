use sealmail_lib::core::{self, imap_should_delete_missing_uid, pop3_inbox_rows_gone_from_server};
use sealmail_lib::models::*;
use sealmail_lib::store::StoreData;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TEMP_ID: AtomicUsize = AtomicUsize::new(0);

fn temp_config_dir(test_name: &str) -> PathBuf {
    let unique = NEXT_TEMP_ID.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "sealmail-{test_name}-{}-{nanos}-{unique}",
        std::process::id()
    ))
}

fn load_store(test_name: &str) -> (PathBuf, StoreData) {
    let dir = temp_config_dir(test_name);
    let store = StoreData::load(dir.clone()).expect("store should load from a fresh temp dir");
    (dir, store)
}

#[test]
fn updating_one_secret_preserves_accounts_added_by_another_process() {
    let (dir, mut stale_gui_store) = load_store("merge-secret-update");
    stale_gui_store.secrets.insert(
        "exchange".into(),
        AccountSecret {
            password: "old-token".into(),
            smtp_password: None,
            oauth: None,
        },
    );
    stale_gui_store.save_secrets().expect("save initial secret");

    let mut cli_store = StoreData::load(dir.clone()).expect("CLI loads current secrets");
    cli_store.secrets.insert(
        "qq".into(),
        AccountSecret {
            password: "qq-authorization-code".into(),
            smtp_password: None,
            oauth: None,
        },
    );
    cli_store
        .save_secrets()
        .expect("CLI saves newly added account");

    stale_gui_store
        .update_secret(
            "exchange",
            AccountSecret {
                password: "refreshed-token".into(),
                smtp_password: None,
                oauth: None,
            },
        )
        .expect("GUI updates only the refreshed account");

    let reloaded = StoreData::load(dir.clone()).expect("reload merged secrets");
    assert_eq!(
        reloaded.secret("exchange").unwrap().password,
        "refreshed-token"
    );
    assert_eq!(
        reloaded.secret("qq").unwrap().password,
        "qq-authorization-code",
        "refreshing stale GUI state must not delete a credential added by the CLI"
    );

    fs::remove_dir_all(dir).ok();
}

/// 凭据损坏/钥匙串优先：保存后即使 secrets.json 被截断，加载仍应恢复出真凭据（钥匙串或备份）。
#[test]
fn corrupt_secrets_json_must_not_load_as_empty() {
    let (dir, mut store) = load_store("corrupt-secrets-load");
    store.secrets.insert(
        "acc1".into(),
        AccountSecret {
            password: "must-not-be-wiped".into(),
            smtp_password: None,
            oauth: None,
        },
    );
    store.save_secrets().expect("seed secrets");

    // 模拟断电/截断导致的损坏 JSON（钥匙串路径下不应再静默变成空表）
    fs::write(dir.join("secrets.json"), "{ \"acc1\": this is not valid json")
        .expect("write corrupt secrets");

    match StoreData::load(dir.clone()) {
        Ok(reloaded) => {
            assert_eq!(
                reloaded.secret("acc1").unwrap().password,
                "must-not-be-wiped",
                "keychain/file recovery must keep credentials"
            );
        }
        Err(err) => {
            // 纯文件后端：必须失败且备份，不能空表落盘
            assert!(
                err.contains("secrets") || err.contains("凭据") || err.contains("解析"),
                "error should mention secrets/parse failure: {err}"
            );
            let backups: Vec<_> = fs::read_dir(&dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.contains("secrets") && n.contains("corrupt"))
                .collect();
            assert!(!backups.is_empty(), "expected a .corrupt backup");
        }
    }

    fs::remove_dir_all(dir).ok();
}
#[test]
fn reload_accounts_from_disk_picks_up_cli_added_account() {
    let (dir, mut gui_store) = load_store("reload-accounts");
    // CLI 进程写入新账户
    let mut cli = StoreData::load(dir.clone()).expect("cli load");
    cli.accounts.push(sample_account("new-acc", "new@example.test"));
    cli.secrets.insert(
        "new-acc".into(),
        AccountSecret {
            password: "secret".into(),
            smtp_password: None,
            oauth: None,
        },
    );
    cli.save_accounts().unwrap();
    cli.save_secrets().unwrap();

    assert!(gui_store.account("new-acc").is_err());
    gui_store
        .reload_accounts_and_secrets_from_disk()
        .expect("reload");
    assert_eq!(gui_store.account("new-acc").unwrap().email, "new@example.test");
    assert_eq!(gui_store.secret("new-acc").unwrap().password, "secret");
    fs::remove_dir_all(dir).ok();
}

#[test]
fn pop3_server_uidl_gone_only_deletes_inbox_not_archive() {
    let uidl_gone = "uidl-archived".to_string();
    let uidl_inbox = "uidl-inbox".to_string();
    let known = vec![
        (uidl_gone.clone(), "归档".into(), 10u32),
        (uidl_inbox.clone(), "INBOX".into(), 11u32),
        ("uidl-still-on-server".into(), "INBOX".into(), 12u32),
    ];
    let on_server = "uidl-still-on-server".to_string();
    let server: HashSet<&String> = HashSet::from([&on_server]);
    let gone = pop3_inbox_rows_gone_from_server(&known, &server);
    assert_eq!(gone, vec![("INBOX".into(), 11)]);
}

#[test]
fn imap_delete_skips_uids_newer_than_sync_start() {
    // 并发：对方刚插入 uid=200，本进程开始时 max=100，FLAGS 快照没有 200
    assert!(!imap_should_delete_missing_uid(200, 100, false));
    // 本进程开始前就知道的 uid，服务器已删 → 应删本地
    assert!(imap_should_delete_missing_uid(50, 100, false));
    // 服务器仍有 → 不删
    assert!(!imap_should_delete_missing_uid(50, 100, true));
}

/// 配置落盘必须是原子替换：成功后不应残留 .tmp，且重载内容完整。
#[test]
fn secrets_write_is_atomic_and_reloadable() {
    let (dir, mut store) = load_store("atomic-secrets-write");
    store.secrets.insert(
        "acc1".into(),
        AccountSecret {
            password: "p@ss".into(),
            smtp_password: Some("smtp".into()),
            oauth: None,
        },
    );
    store.save_secrets().expect("atomic save");

    assert!(dir.join("secrets.json").exists());
    assert!(
        !dir.join("secrets.json.tmp").exists(),
        "temp file must be renamed away after success"
    );
    let reloaded = StoreData::load(dir.clone()).expect("reload after atomic write");
    assert_eq!(reloaded.secret("acc1").unwrap().password, "p@ss");
    assert_eq!(
        reloaded.secret("acc1").unwrap().smtp_password.as_deref(),
        Some("smtp")
    );

    fs::remove_dir_all(dir).ok();
}

fn sample_account(id: &str, email: &str) -> Account {
    Account {
        id: id.into(),
        label: "Work".into(),
        email: email.into(),
        display_name: "Mara".into(),
        protocol: IncomingProtocol::Imap,
        incoming_host: "imap.example.test".into(),
        incoming_port: 993,
        smtp_host: "smtp.example.test".into(),
        smtp_port: 465,
        smtp_security: "ssl".into(),
        username: email.into(),
        auth: "password".into(),
    }
}

#[test]
fn state_view_exposes_public_state_without_secrets() {
    let (dir, mut store) = load_store("state-view");
    store
        .accounts
        .push(sample_account("acc1", "mara@example.test"));
    store.secrets.insert(
        "acc1".into(),
        AccountSecret {
            password: "real-password".into(),
            smtp_password: Some("smtp-secret".into()),
            oauth: None,
        },
    );
    store.trusted.push(TrustedContact {
        name: "Aria".into(),
        email: "aria@example.test".into(),
        fingerprint: "AAAA BBBB".into(),
        org: Some("Seal".into()),
        since: "2026-01-01".into(),
        verified_count: 7,
    });
    store.filters.push(FilterRule {
        id: "filter1".into(),
        name: "Invoices".into(),
        account_id: None,
        field: "subject".into(),
        op: "contains".into(),
        value: "invoice".into(),
        target_folder: "Finance".into(),
        mark_read: true,
        enabled: true,
    });
    store.local_folders.push("Finance".into());

    let view = core::state_view(&store);
    assert_eq!(view.accounts.len(), 1);
    assert_eq!(view.accounts[0].email, "mara@example.test");
    assert_eq!(view.trusted.len(), 1);
    assert_eq!(view.filters.len(), 1);
    assert_eq!(view.local_folders, vec!["Finance"]);
    assert_eq!(view.identity.mode, "local");
    assert!(!view.identity.fingerprint.is_empty());

    let json = serde_json::to_string(&view).expect("state view should serialize");
    assert!(!json.contains("real-password"));
    assert!(!json.contains("smtp-secret"));

    fs::remove_dir_all(dir).ok();
}

#[test]
fn identity_and_preferences_roundtrip_to_disk() {
    let (dir, mut store) = load_store("identity-prefs");

    let ledger = core::bind_ledger(
        &mut store,
        "m/44'/60'/0'/0/0".into(),
        "0xAbCDEF0000000000000000000000000000000000".into(),
    )
    .expect("ledger identity should save");
    assert_eq!(ledger.mode, "ledger");
    assert_eq!(
        ledger.ledger_address.as_deref(),
        Some("0xabcdef0000000000000000000000000000000000")
    );

    assert_eq!(
        core::set_close_behavior(&mut store, "quit".into()).expect("valid close behavior"),
        "quit"
    );
    assert!(core::set_close_behavior(&mut store, "minimize".into()).is_err());
    assert!(!core::set_notify_new_mail(&mut store, false).expect("notify pref should save"));

    let reloaded = StoreData::load(dir.clone()).expect("store should reload saved prefs");
    assert_eq!(reloaded.identity_config.mode, "ledger");
    assert_eq!(
        reloaded.identity_config.ledger_address.as_deref(),
        Some("0xabcdef0000000000000000000000000000000000")
    );
    assert_eq!(reloaded.prefs.close_behavior, "quit");
    assert!(!reloaded.prefs.notify_new_mail);

    core::use_local_key(&mut store).expect("local identity should save");
    let reloaded = StoreData::load(dir.clone()).expect("store should reload local identity mode");
    assert_eq!(reloaded.identity_config.mode, "local");
    assert!(reloaded.identity_config.ledger_address.is_none());

    fs::remove_dir_all(dir).ok();
}

#[test]
fn contacts_are_filtered_and_sorted_by_strength_then_recency() {
    let (dir, mut store) = load_store("contacts");
    store.contacts.insert(
        "aria@example.test".into(),
        Contact {
            name: "Aria".into(),
            email: "aria@example.test".into(),
            last_seen: 100,
            count: 2,
        },
    );
    store.contacts.insert(
        "mara@example.test".into(),
        Contact {
            name: "Mara".into(),
            email: "mara@example.test".into(),
            last_seen: 200,
            count: 4,
        },
    );
    store.contacts.insert(
        "zara@example.test".into(),
        Contact {
            name: "Zara".into(),
            email: "zara@example.test".into(),
            last_seen: 300,
            count: 4,
        },
    );

    let all = core::list_contacts(&store, None);
    let emails: Vec<&str> = all.iter().map(|c| c.email.as_str()).collect();
    assert_eq!(
        emails,
        vec![
            "zara@example.test",
            "mara@example.test",
            "aria@example.test"
        ]
    );

    let filtered = core::list_contacts(&store, Some("mar".into()));
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].email, "mara@example.test");

    fs::remove_dir_all(dir).ok();
}

#[test]
fn drafts_filters_and_trusted_contacts_are_persisted_with_overwrite_semantics() {
    let (dir, mut store) = load_store("local-workflows");

    let first = core::save_draft(
        &mut store,
        Draft {
            id: "draft1".into(),
            account_id: "acc1".into(),
            to: "aria@example.test".into(),
            cc: String::new(),
            subject: "Old subject".into(),
            body: "old".into(),
            sign: true,
            attachment_paths: vec!["/tmp/a.pdf".into()],
            updated_at: 1,
        },
    )
    .expect("draft should save");
    let second = core::save_draft(
        &mut store,
        Draft {
            subject: "New subject".into(),
            body: "new".into(),
            ..first.clone()
        },
    )
    .expect("draft overwrite should save");
    assert_eq!(second.id, "draft1");
    let drafts = core::list_drafts(&store);
    assert_eq!(drafts.len(), 1);
    assert_eq!(drafts[0].subject, "New subject");
    assert_eq!(drafts[0].body, "new");
    assert!(drafts[0].updated_at >= first.updated_at);

    let filters = core::save_filter(
        &mut store,
        FilterRule {
            id: "rule1".into(),
            name: "Invoices".into(),
            account_id: Some("acc1".into()),
            field: "subject".into(),
            op: "contains".into(),
            value: "invoice".into(),
            target_folder: "Finance".into(),
            mark_read: true,
            enabled: true,
        },
    )
    .expect("filter should save");
    assert_eq!(filters.len(), 1);
    assert_eq!(filters[0].target_folder, "Finance");
    let filters = core::save_filter(
        &mut store,
        FilterRule {
            target_folder: "Accounting".into(),
            ..filters[0].clone()
        },
    )
    .expect("filter overwrite should save");
    assert_eq!(filters.len(), 1);
    assert_eq!(filters[0].target_folder, "Accounting");

    let trusted = core::trust_sender(
        &mut store,
        "Aria".into(),
        "Aria@Example.Test".into(),
        "AAAA BBBB".into(),
        None,
    )
    .expect("trusted contact should save");
    assert_eq!(trusted.len(), 1);
    assert_eq!(trusted[0].verified_count, 1);
    let trusted = core::trust_sender(
        &mut store,
        "Aria New".into(),
        "aria@example.test".into(),
        "CCCC DDDD".into(),
        Some("Seal".into()),
    )
    .expect("trusted contact overwrite should save");
    assert_eq!(trusted.len(), 1);
    assert_eq!(trusted[0].name, "Aria New");
    assert_eq!(trusted[0].fingerprint, "CCCC DDDD");

    let reloaded = StoreData::load(dir.clone()).expect("store should reload local workflows");
    assert_eq!(reloaded.drafts.len(), 1);
    assert_eq!(reloaded.drafts[0].subject, "New subject");
    assert_eq!(reloaded.filters.len(), 1);
    assert_eq!(reloaded.filters[0].target_folder, "Accounting");
    assert_eq!(reloaded.trusted.len(), 1);
    assert_eq!(reloaded.trusted[0].fingerprint, "CCCC DDDD");

    core::delete_draft(&mut store, "draft1".into()).expect("draft should delete");
    core::delete_filter(&mut store, "rule1".into()).expect("filter should delete");
    core::remove_trusted(&mut store, "ARIA@EXAMPLE.TEST".into())
        .expect("trusted contact should delete case-insensitively");

    let reloaded = StoreData::load(dir.clone()).expect("store should reload deletions");
    assert!(reloaded.drafts.is_empty());
    assert!(reloaded.filters.is_empty());
    assert!(reloaded.trusted.is_empty());

    fs::remove_dir_all(dir).ok();
}

/// 「屏蔽发件人」按钮多次点击会重复提交内容完全相同的新规则；
/// 保存时应去重（更新现有规则）而不是无限追加。
#[test]
fn saving_identical_new_rule_is_deduplicated() {
    let (dir, mut store) = load_store("filter-dedupe");
    let rule = FilterRule {
        id: String::new(),
        name: "屏蔽 support@vultr.com".into(),
        account_id: Some("acc1".into()),
        field: "from".into(),
        op: "contains".into(),
        value: "support@vultr.com".into(),
        target_folder: "&V4NXPpCuTvY-".into(),
        mark_read: true,
        enabled: true,
    };
    let filters = core::save_filter(&mut store, rule.clone()).expect("first save");
    assert_eq!(filters.len(), 1);
    let first_id = filters[0].id.clone();

    let filters = core::save_filter(&mut store, rule.clone()).expect("duplicate save");
    assert_eq!(filters.len(), 1, "相同匹配条件+目标的新规则不应重复追加");
    assert_eq!(filters[0].id, first_id, "应更新现有规则而不是新建");

    // 匹配条件不同的规则仍然可以新增
    let filters = core::save_filter(
        &mut store,
        FilterRule {
            value: "billing@vultr.com".into(),
            ..rule.clone()
        },
    )
    .expect("different rule saves");
    assert_eq!(filters.len(), 2);

    fs::remove_dir_all(dir).ok();
}

/// 缺 meta_json 时 list_cached 返回「…」占位；backfill 后应写出真实主题/发件人。
/// 回归：v0.1.56 把解析挪到后台后，若不同步写 meta、也不通知前端，列表会永久显示 …。
#[test]
fn list_cached_stub_then_backfill_fills_subject() {
    use sealmail_lib::db;

    let (dir, mut store) = load_store("meta-stub-backfill");
    store
        .accounts
        .push(sample_account("acc1", "mara@example.test"));

    let raw = b"From: Alice <alice@example.test>\r\n\
Subject: Hello from Alice\r\n\
Date: Tue, 14 Jul 2026 10:00:00 +0000\r\n\
Message-ID: <stub-test@example.test>\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Body preview text here.\r\n";
    db::upsert_message(
        &store.db,
        "acc1",
        "INBOX",
        42,
        None,
        true,
        false,
        1_721_000_000,
        raw,
    )
    .expect("insert raw without meta_json");

    let list = core::list_cached(&mut store, "acc1", "INBOX", 0, 0).expect("list");
    assert_eq!(list.metas.len(), 1);
    assert!(
        core::is_stub_meta(&list.metas[0]),
        "missing meta_json must surface as stub placeholder"
    );
    assert_eq!(list.metas[0].subject, "…");
    assert_eq!(list.metas[0].from_name, "…");

    let (n, max_uid) =
        core::backfill_meta_batch(&mut store, "acc1", "INBOX", 0, 40).expect("backfill");
    assert_eq!(n, 1);
    assert_eq!(max_uid, Some(42));

    let list = core::list_cached(&mut store, "acc1", "INBOX", 0, 0).expect("list after backfill");
    assert_eq!(list.metas.len(), 1);
    assert!(
        !core::is_stub_meta(&list.metas[0]),
        "backfill must replace stub with real meta"
    );
    assert_eq!(list.metas[0].subject, "Hello from Alice");
    assert_eq!(list.metas[0].from_name, "Alice");
    assert_eq!(list.metas[0].from_addr, "alice@example.test");

    fs::remove_dir_all(dir).ok();
}

/// 点通知时邮件可能已被过滤规则移出 INBOX：要能按 Message-ID 在本地缓存
/// 全目录范围内定位它现在所在的目录和 UID。
#[test]
fn locate_in_db_finds_mail_moved_out_of_inbox() {
    let (dir, mut store) = load_store("locate-by-msgid");

    // 原始头里的 Message-ID 可能是大写（如 Outlook），而通知目标里的 id 已被规范化为小写，
    // 定位必须大小写不敏感
    let moved = b"Message-ID: <BUILD-42@wanchain.org>\r\nFrom: jenkins@wanchain.org\r\nSubject: build ok\r\n\r\nbody".to_vec();
    // 同一 Message-ID 出现在另一封邮件的 References 里（回复），不能被误定位
    let reply = b"Message-ID: <re-1@example.test>\r\nReferences: <build-42@wanchain.org>\r\nFrom: mara@example.test\r\nSubject: Re: build ok\r\n\r\nreply".to_vec();
    sealmail_lib::db::upsert_message(&store.db, "acc1", "&ZzpWaE66-", 90, None, false, false, 2000, &moved)
        .expect("insert moved mail");
    sealmail_lib::db::upsert_message(&store.db, "acc1", "INBOX", 500, None, true, false, 3000, &reply)
        .expect("insert reply mail");

    let loc = core::locate_in_db(&mut store, "acc1", "build-42@wanchain.org")
        .expect("locate should not error")
        .expect("mail should be found by message id");
    assert_eq!(loc.folder, "&ZzpWaE66-");
    assert_eq!(loc.uid, 90);

    assert!(
        core::locate_in_db(&mut store, "acc1", "no-such-id@example.test")
            .expect("locate should not error")
            .is_none(),
        "不存在的 Message-ID 应返回 None"
    );

    fs::remove_dir_all(dir).ok();
}

#[test]
fn truncated_empty_accounts_json_blocks_load_with_backup() {
    // 空文件 = 写入被截断的典型产物;静默当默认值会把仅存线索覆盖掉
    let dir = temp_config_dir("empty-accounts");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("accounts.json"), "").unwrap();
    let err = match StoreData::load(dir.clone()) {
        Ok(_) => panic!("空 accounts.json 必须报错"),
        Err(e) => e,
    };
    assert!(err.contains("accounts.json"), "{err}");
    let backups: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("accounts") && n.contains("corrupt"))
        .collect();
    assert!(!backups.is_empty(), "应留下 .corrupt 备份");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn corrupt_noncritical_contacts_json_is_backed_up_and_recovers() {
    let (dir, store) = load_store("corrupt-contacts-recovery");
    drop(store);
    std::fs::write(dir.join("contacts.json"), "{not valid json").unwrap();

    let reloaded = StoreData::load(dir.clone())
        .expect("a corrupt contacts cache must not prevent the app from starting");
    assert!(reloaded.contacts.is_empty());
    let backups: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("contacts") && n.contains("corrupt"))
        .collect();
    assert!(!backups.is_empty(), "应留下 contacts.json.corrupt 备份");
    let _ = std::fs::remove_dir_all(&dir);
}
