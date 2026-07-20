use sealmail_lib::models::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TEMP_ID: AtomicUsize = AtomicUsize::new(0);

fn cli_bin() -> &'static str {
    env!("CARGO_BIN_EXE_sealmail-cli")
}

fn gui_bin() -> &'static str {
    env!("CARGO_BIN_EXE_sealmail")
}

fn temp_config_dir(test_name: &str) -> PathBuf {
    let unique = NEXT_TEMP_ID.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "sealmail-cli-{test_name}-{}-{nanos}-{unique}",
        std::process::id()
    ))
}

fn run_cli(dir: &Path, args: &[&str]) -> Output {
    Command::new(cli_bin())
        .args(args)
        .env("SEALMAIL_CONFIG_DIR", dir)
        // 契约断言的是中文文案；宿主 LANG 可能是英文（language 默认跟随系统），固定语言保证确定性
        .env("LC_ALL", "zh_CN.UTF-8")
        .output()
        .expect("cli process should start")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout should be utf8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr should be utf8")
}

fn write_json<T: serde::Serialize>(dir: &Path, name: &str, value: &T) {
    fs::create_dir_all(dir).expect("config dir should be created");
    let json = serde_json::to_string_pretty(value).expect("fixture should serialize");
    fs::write(dir.join(name), json).expect("fixture should write");
}

#[test]
fn gui_binary_uses_gui_entrypoint_by_default() {
    let output = Command::new(gui_bin())
        .arg("--sealmail-gui-entry-smoke")
        .output()
        .expect("gui process should start");

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output).trim(), "sealmail-gui-entry");
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

fn seed_config(dir: &Path) {
    write_json(
        dir,
        "accounts.json",
        &vec![sample_account("acc1", "mara@example.test")],
    );
    let mut secrets = std::collections::HashMap::new();
    secrets.insert(
        "acc1".to_string(),
        AccountSecret {
            password: "real-password".into(),
            smtp_password: Some("smtp-secret".into()),
            oauth: None,
        },
    );
    write_json(dir, "secrets.json", &secrets);
    write_json(
        dir,
        "contacts.json",
        &std::collections::HashMap::from([(
            "aria@example.test".to_string(),
            Contact {
                name: "Aria".into(),
                email: "aria@example.test".into(),
                last_seen: 12,
                count: 3,
            },
        )]),
    );
    write_json(
        dir,
        "drafts.json",
        &vec![Draft {
            id: "draft1".into(),
            account_id: "acc1".into(),
            to: "aria@example.test".into(),
            cc: String::new(),
            subject: "Hello".into(),
            body: "Body".into(),
            sign: true,
            attachment_paths: Vec::new(),
            updated_at: 42,
        }],
    );
    write_json(
        dir,
        "trusted.json",
        &vec![TrustedContact {
            name: "Aria".into(),
            email: "aria@example.test".into(),
            fingerprint: "AAAA BBBB".into(),
            org: None,
            since: "2026-01-01".into(),
            verified_count: 1,
        }],
    );
    write_json(
        dir,
        "filters.json",
        &vec![FilterRule {
            id: "rule1".into(),
            name: "Invoices".into(),
            account_id: Some("acc1".into()),
            field: "subject".into(),
            op: "contains".into(),
            value: "invoice".into(),
            target_folder: "Finance".into(),
            mark_read: true,
            enabled: true,
        }],
    );
}

#[test]
fn help_and_unknown_command_have_distinct_exit_contracts() {
    let dir = temp_config_dir("help");

    let help = run_cli(&dir, &["help"]);
    assert!(help.status.success());
    let help_out = stdout(&help);
    assert!(help_out.contains("USAGE:"));
    assert!(help_out.contains("state"));
    assert!(help_out.contains("account add"));
    assert!(help_out.contains("sync-older"));
    assert!(help_out.contains("attachment save"));
    assert!(help_out.contains("attachment data"));
    assert!(help_out.contains("draft save"));
    assert!(help_out.contains("filter apply"));
    assert!(help_out.contains("trust add"));
    assert!(help_out.contains("identity bind-ledger"));
    assert!(help_out.contains("pref set"));
    assert!(help_out.contains("filters"));

    let bad = run_cli(&dir, &["does-not-exist"]);
    assert!(!bad.status.success());
    assert!(stderr(&bad).contains("未知命令: does-not-exist"));

    fs::remove_dir_all(dir).ok();
}

#[test]
fn local_user_commands_persist_drafts_filters_and_trusted_contacts() {
    let dir = temp_config_dir("local-user-commands");
    seed_config(&dir);

    let draft = run_cli(
        &dir,
        &[
            "draft",
            "save",
            "--id",
            "draft2",
            "--account",
            "acc1",
            "--to",
            "nova@example.test",
            "--subject",
            "Draft from CLI",
            "--body",
            "Saved by CLI",
            "--json",
        ],
    );
    assert!(draft.status.success(), "stderr: {}", stderr(&draft));
    let draft_json: serde_json::Value =
        serde_json::from_str(&stdout(&draft)).expect("draft save should return json");
    assert_eq!(draft_json["id"], "draft2");
    assert_eq!(draft_json["subject"], "Draft from CLI");

    let drafts = run_cli(&dir, &["drafts", "--json"]);
    assert!(drafts.status.success());
    let drafts_json: serde_json::Value =
        serde_json::from_str(&stdout(&drafts)).expect("drafts should be json");
    assert!(drafts_json
        .as_array()
        .unwrap()
        .iter()
        .any(|draft| draft["id"] == "draft2"));

    let filter = run_cli(
        &dir,
        &[
            "filter",
            "save",
            "--id",
            "rule2",
            "--account",
            "acc1",
            "--name",
            "Receipts",
            "--field",
            "subject",
            "--op",
            "contains",
            "--value",
            "receipt",
            "--target",
            "Receipts",
            "--mark-read",
            "true",
            "--json",
        ],
    );
    assert!(filter.status.success(), "stderr: {}", stderr(&filter));
    let filters_json: serde_json::Value =
        serde_json::from_str(&stdout(&filter)).expect("filter save should return json");
    assert!(filters_json
        .as_array()
        .unwrap()
        .iter()
        .any(|rule| rule["id"] == "rule2" && rule["targetFolder"] == "Receipts"));

    let trust = run_cli(
        &dir,
        &[
            "trust",
            "add",
            "--name",
            "Nova",
            "--email",
            "nova@example.test",
            "--fingerprint",
            "FFFF 1111",
            "--org",
            "Seal",
            "--json",
        ],
    );
    assert!(trust.status.success(), "stderr: {}", stderr(&trust));
    let trusted_json: serde_json::Value =
        serde_json::from_str(&stdout(&trust)).expect("trust add should return json");
    assert!(trusted_json
        .as_array()
        .unwrap()
        .iter()
        .any(|contact| contact["email"] == "nova@example.test"
            && contact["fingerprint"] == "FFFF 1111"));

    let delete_draft = run_cli(&dir, &["draft", "delete", "--id", "draft2", "--json"]);
    assert!(
        delete_draft.status.success(),
        "stderr: {}",
        stderr(&delete_draft)
    );
    let delete_filter = run_cli(&dir, &["filter", "delete", "--id", "rule2", "--json"]);
    assert!(
        delete_filter.status.success(),
        "stderr: {}",
        stderr(&delete_filter)
    );
    let remove_trust = run_cli(
        &dir,
        &["trust", "remove", "--email", "nova@example.test", "--json"],
    );
    assert!(
        remove_trust.status.success(),
        "stderr: {}",
        stderr(&remove_trust)
    );

    fs::remove_dir_all(dir).ok();
}

#[test]
fn local_settings_commands_persist_identity_and_preferences() {
    let dir = temp_config_dir("local-settings-commands");

    let bind = run_cli(
        &dir,
        &[
            "identity",
            "bind-ledger",
            "--ledger-path",
            "m/44'/60'/0'/0/0",
            "--address",
            "0xAbCDEF0000000000000000000000000000000000",
            "--json",
        ],
    );
    assert!(bind.status.success(), "stderr: {}", stderr(&bind));
    let bind_json: serde_json::Value =
        serde_json::from_str(&stdout(&bind)).expect("identity bind should return json");
    assert_eq!(bind_json["mode"], "ledger");
    assert_eq!(
        bind_json["ledgerAddress"],
        "0xabcdef0000000000000000000000000000000000"
    );

    let prefs = run_cli(
        &dir,
        &[
            "pref",
            "set",
            "--close-behavior",
            "quit",
            "--notify-new-mail",
            "false",
            "--json",
        ],
    );
    assert!(prefs.status.success(), "stderr: {}", stderr(&prefs));
    let prefs_json: serde_json::Value =
        serde_json::from_str(&stdout(&prefs)).expect("pref set should return json");
    assert_eq!(prefs_json["closeBehavior"], "quit");
    assert_eq!(prefs_json["notifyNewMail"], false);

    let state = run_cli(&dir, &["state", "--json"]);
    assert!(state.status.success(), "stderr: {}", stderr(&state));
    let state_json: serde_json::Value =
        serde_json::from_str(&stdout(&state)).expect("state should be json");
    assert_eq!(state_json["identity"]["mode"], "ledger");

    let local = run_cli(&dir, &["identity", "use-local", "--json"]);
    assert!(local.status.success(), "stderr: {}", stderr(&local));
    let local_json: serde_json::Value =
        serde_json::from_str(&stdout(&local)).expect("identity use-local should return json");
    assert_eq!(local_json["mode"], "local");
    assert!(local_json["ledgerAddress"].is_null());

    fs::remove_dir_all(dir).ok();
}

#[test]
fn config_dir_uses_environment_override_without_initializing_store() {
    let dir = temp_config_dir("config-dir");
    let output = run_cli(&dir, &["config-dir"]);
    assert!(output.status.success());
    assert_eq!(stdout(&output).trim(), dir.display().to_string());
    assert!(
        !dir.exists(),
        "config-dir should not create identity or db files"
    );
}

#[test]
fn state_json_is_valid_camel_case_and_does_not_expose_secrets() {
    let dir = temp_config_dir("state-json");
    seed_config(&dir);

    let output = run_cli(&dir, &["state", "--json"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let text = stdout(&output);
    let value: serde_json::Value = serde_json::from_str(&text).expect("state should be json");

    assert_eq!(value["accounts"][0]["email"], "mara@example.test");
    assert!(value["identity"]["fingerprint"].as_str().unwrap().len() > 8);
    assert_eq!(value["trusted"][0]["verifiedCount"], 1);
    assert_eq!(value["filters"][0]["targetFolder"], "Finance");
    assert!(value.get("localFolders").is_some());
    assert!(value.get("local_folders").is_none());
    assert!(!text.contains("real-password"));
    assert!(!text.contains("smtp-secret"));

    fs::remove_dir_all(dir).ok();
}

#[test]
fn read_only_json_commands_return_expected_public_shapes() {
    let dir = temp_config_dir("read-only-json");
    seed_config(&dir);

    let accounts = run_cli(&dir, &["accounts", "--json"]);
    assert!(accounts.status.success());
    let accounts_text = stdout(&accounts);
    let accounts_json: serde_json::Value =
        serde_json::from_str(&accounts_text).expect("accounts should be json");
    assert_eq!(accounts_json[0]["id"], "acc1");
    assert_eq!(accounts_json[0]["username"], "mara@example.test");
    assert!(!accounts_text.contains("real-password"));
    assert!(!accounts_text.contains("smtp-secret"));

    let contacts = run_cli(&dir, &["contacts", "--json"]);
    assert!(contacts.status.success());
    let contacts_json: serde_json::Value =
        serde_json::from_str(&stdout(&contacts)).expect("contacts should be json");
    assert_eq!(contacts_json[0]["email"], "aria@example.test");
    assert_eq!(contacts_json[0]["lastSeen"], 12);

    let drafts = run_cli(&dir, &["drafts", "--json"]);
    assert!(drafts.status.success());
    let drafts_json: serde_json::Value =
        serde_json::from_str(&stdout(&drafts)).expect("drafts should be json");
    assert_eq!(drafts_json[0]["id"], "draft1");
    assert_eq!(drafts_json[0]["accountId"], "acc1");

    let trusted = run_cli(&dir, &["trusted", "--json"]);
    assert!(trusted.status.success());
    let trusted_json: serde_json::Value =
        serde_json::from_str(&stdout(&trusted)).expect("trusted should be json");
    assert_eq!(trusted_json[0]["fingerprint"], "AAAA BBBB");

    let filters = run_cli(&dir, &["filters", "--json"]);
    assert!(filters.status.success());
    let filters_json: serde_json::Value =
        serde_json::from_str(&stdout(&filters)).expect("filters should be json");
    assert_eq!(filters_json[0]["targetFolder"], "Finance");

    fs::remove_dir_all(dir).ok();
}

#[test]
fn empty_config_state_initializes_local_identity_and_cache() {
    let dir = temp_config_dir("empty-state");
    let output = run_cli(&dir, &["state", "--json"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let value: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("state should be json");

    assert_eq!(value["accounts"].as_array().unwrap().len(), 0);
    assert_eq!(value["identity"]["mode"], "local");
    assert!(value["identity"]["publicKey"].as_str().unwrap().len() > 20);
    assert!(dir.join("identity.key").exists());
    assert!(dir.join("mail.db").exists());

    fs::remove_dir_all(dir).ok();
}

#[test]
fn account_add_requires_password_env_before_saving_anything() {
    let dir = temp_config_dir("account-add-missing-password");
    let output = Command::new(cli_bin())
        .args([
            "account",
            "add",
            "--id",
            "acc1",
            "--email",
            "mara@example.test",
            "--protocol",
            "imap",
            "--incoming-host",
            "imap.example.test",
            "--incoming-port",
            "993",
            "--smtp-host",
            "smtp.example.test",
            "--smtp-port",
            "465",
            "--smtp-security",
            "ssl",
        ])
        .env("SEALMAIL_CONFIG_DIR", &dir)
        .env_remove("SEALMAIL_PASSWORD")
        .env_remove("SEALMAIL_SMTP_PASSWORD")
        .output()
        .expect("cli process should start");

    assert!(!output.status.success());
    assert!(stderr(&output).contains("SEALMAIL_PASSWORD"));
    assert!(
        !dir.join("accounts.json").exists(),
        "failed account add must not save account config"
    );
    assert!(
        !dir.join("secrets.json").exists(),
        "failed account add must not save secrets"
    );

    fs::remove_dir_all(dir).ok();
}

#[test]
fn account_remove_deletes_account_and_secret() {
    let dir = temp_config_dir("account-remove");
    seed_config(&dir);

    let output = run_cli(&dir, &["account", "remove", "--id", "acc1", "--json"]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let removed: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("remove should return json");
    assert_eq!(removed["removed"], "acc1");

    let accounts = run_cli(&dir, &["accounts", "--json"]);
    assert!(accounts.status.success());
    let accounts_json: serde_json::Value =
        serde_json::from_str(&stdout(&accounts)).expect("accounts should be json");
    assert_eq!(accounts_json.as_array().unwrap().len(), 0);

    // 凭据可能在钥匙串：磁盘上是空表或 keychain 占位；以 StoreData 为准确认已清空
    let reloaded = sealmail_lib::store::StoreData::load(dir.clone()).expect("reload after remove");
    assert!(
        reloaded.secrets.is_empty(),
        "account remove must clear all stored secrets"
    );
    assert!(reloaded.accounts.is_empty());

    fs::remove_dir_all(dir).ok();
}
