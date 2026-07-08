//! 核心逻辑端到端测试：签名 → 构造真实 MIME → 解析 → 验证各信任状态。

use base64::{engine::general_purpose::STANDARD, Engine as _};
use encoding_rs::GB18030;
use mail_builder::headers::raw::Raw;
use mail_builder::MessageBuilder;
use sealmail_lib::crypto::{self, Identity};
use sealmail_lib::filters::rule_matches;
use sealmail_lib::mail::{detect_lang, detect_risk, parse_email};
use sealmail_lib::models::*;

fn test_identity() -> Identity {
    let seed = [7u8; 32];
    Identity {
        signing_key: ed25519_dalek::SigningKey::from_bytes(&seed),
        created: "2026-01-01T00:00:00Z".into(),
    }
}

/// 构造一封（可选签名的）原始邮件
fn build_raw(
    from_name: &str,
    from_addr: &str,
    subject: &str,
    body: &str,
    identity: Option<&Identity>,
) -> Vec<u8> {
    let mut b = MessageBuilder::new()
        .from((from_name, from_addr))
        .to(vec![("", "aria@example.com")])
        .subject(subject)
        .text_body(body);
    if let Some(id) = identity {
        for (name, value) in crypto::sign_email(id, from_addr, body).headers {
            b = b.header(name, Raw::new(value));
        }
    }
    b.write_to_vec().unwrap()
}

fn trusted_for(identity: &Identity, name: &str, email: &str) -> Vec<TrustedContact> {
    vec![TrustedContact {
        name: name.into(),
        email: email.into(),
        fingerprint: identity.fingerprint(),
        org: None,
        since: "2025-01-01".into(),
        verified_count: 42,
    }]
}

#[test]
fn sign_then_verify_roundtrip() {
    let id = test_identity();
    let body = "Hello Aria,\r\n\r\nPlease review the doc.\r\n";
    let signed = crypto::sign_email(&id, "mara@example.com", body);
    let get = |k: &str| {
        signed
            .headers
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.clone())
            .unwrap()
    };
    let h = crypto::SealHeaders {
        pubkey: get(crypto::H_PUBKEY),
        signature: get(crypto::H_SIGNATURE),
        from: get(crypto::H_FROM),
        date: get(crypto::H_DATE),
        body_hash: get(crypto::H_BODY_HASH),
    };
    // CRLF→LF 等规范化后应当一致
    let (fpr, body_ok, _, _) =
        crypto::verify_headers(&h, "Hello Aria,\n\nPlease review the doc.").unwrap();
    assert!(body_ok, "规范化后的正文哈希必须一致");
    assert_eq!(fpr, id.fingerprint());

    // 正文被篡改 → body_ok = false
    let (_, tampered_ok, signed_hash, got_hash) =
        crypto::verify_headers(&h, "Hello Aria,\n\nPlease send 5000 USDC now.").unwrap();
    assert!(!tampered_ok);
    assert_ne!(signed_hash, got_hash);

    // 签名本身被篡改 → Err
    let mut bad = crypto::SealHeaders {
        pubkey: h.pubkey.clone(),
        signature: h.signature.clone(),
        from: h.from.clone(),
        date: h.date.clone(),
        body_hash: h.body_hash.clone(),
    };
    bad.from = "attacker@evil.com".into();
    assert!(crypto::verify_headers(&bad, "Hello Aria,\n\nPlease review the doc.").is_err());
}

#[test]
fn e2e_verified_mail() {
    let id = test_identity();
    let body = "Quarterly report attached.\nNumbers look good.";
    let raw = build_raw(
        "Mara Castellanos",
        "mara@aragon.eth",
        "Q2 Report",
        body,
        Some(&id),
    );
    let trusted = trusted_for(&id, "Mara Castellanos", "mara@aragon.eth");
    let mail = parse_email(&raw, 1, "acc1", "INBOX", true, false, &trusted).unwrap();
    assert_eq!(mail.meta.trust, "verified");
    match mail.verify {
        VerifyDetail::Verified {
            contact_name,
            verified_count,
            ..
        } => {
            assert_eq!(contact_name, "Mara Castellanos");
            assert_eq!(verified_count, 42);
        }
        other => panic!("应为 Verified，实际 {:?}", other),
    }
}

#[test]
fn e2e_signed_unknown_mail() {
    let id = test_identity();
    let raw = build_raw(
        "New Person",
        "new@startup.io",
        "Intro",
        "Hi, we just met.",
        Some(&id),
    );
    let mail = parse_email(&raw, 2, "acc1", "INBOX", true, false, &[]).unwrap();
    assert_eq!(mail.meta.trust, "signedUnknown");
}

#[test]
fn decodes_legacy_chinese_mail_without_charset() {
    let (from_name, _, _) = GB18030.encode("测试用户");
    let (subject, _, _) = GB18030.encode("修改密码");
    let (body, _, _) = GB18030.encode("两个点\r\nOK了\r\n");
    let mut raw = Vec::new();
    raw.extend_from_slice(b"From: ");
    raw.extend_from_slice(&from_name);
    raw.extend_from_slice(b" <f814326328@163.com>\r\n");
    raw.extend_from_slice(b"To: molin@example.com\r\nSubject: ");
    raw.extend_from_slice(&subject);
    raw.extend_from_slice(
        b"\r\nContent-Type: text/plain\r\nContent-Transfer-Encoding: 8bit\r\n\r\n",
    );
    raw.extend_from_slice(&body);

    let mail = parse_email(&raw, 20, "acc1", "INBOX", true, false, &[]).unwrap();
    assert_eq!(mail.meta.from_name, "测试用户");
    assert_eq!(mail.meta.subject, "修改密码");
    assert!(mail.body_text.contains("两个点"));
    assert_eq!(mail.meta.preview, "两个点");
    assert_eq!(mail.meta.lang, "ZH");
}

#[test]
fn decodes_gbk_encoded_word_headers() {
    let encode_word = |text: &str| {
        let (bytes, _, _) = GB18030.encode(text);
        format!("=?GBK?B?{}?=", STANDARD.encode(bytes))
    };
    let raw = format!(
        "From: {} <f814326328@163.com>\r\nTo: molin@example.com\r\nSubject: {}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n账号 18223506701\r\n密码 fh2012008..\r\n",
        encode_word("账号"),
        encode_word("密码")
    )
    .into_bytes();

    let mail = parse_email(&raw, 21, "acc1", "INBOX", true, false, &[]).unwrap();
    assert_eq!(mail.meta.from_name, "账号");
    assert_eq!(mail.meta.subject, "密码");
    assert_eq!(mail.meta.preview, "账号 18223506701");
}

#[test]
fn decodes_nested_legacy_chinese_body_with_attachments() {
    let (body, _, _) = GB18030.encode("爱乐评留言回复\r\n你于2025年6月17日收到一条新回复。\r\n");
    let body_b64 = STANDARD.encode(body);
    let raw = format!(
        concat!(
            "From: huax1234 <huax1234@163.com>\r\n",
            "To: molin@example.com\r\n",
            "Subject: =?GBK?B?u6rAtLz+yrLDtA==?=\r\n",
            "Content-Type: multipart/mixed; boundary=\"outer\"\r\n\r\n",
            "--outer\r\n",
            "Content-Type: multipart/alternative; boundary=\"alt\"\r\n\r\n",
            "--alt\r\n",
            "Content-Type: text/plain; charset=utf-8\r\n",
            "Content-Transfer-Encoding: base64\r\n\r\n",
            "{}\r\n",
            "--alt--\r\n",
            "--outer\r\n",
            "Content-Type: image/jpeg; name=\"screenshot.jpg\"\r\n",
            "Content-Disposition: attachment; filename=\"screenshot.jpg\"\r\n",
            "Content-Transfer-Encoding: base64\r\n\r\n",
            "/9j/4AAQSkZJRg==\r\n",
            "--outer--\r\n"
        ),
        body_b64
    )
    .into_bytes();

    let mail = parse_email(&raw, 22, "acc1", "INBOX", true, false, &[]).unwrap();
    assert!(mail.body_text.contains("爱乐评留言回复"));
    assert!(mail.body_text.contains("2025年6月17日"));
    assert!(!mail.body_text.contains('\u{FFFD}'));
    assert_eq!(mail.meta.preview, "爱乐评留言回复");
    assert_eq!(mail.attachments.len(), 1);
}

#[test]
fn parses_conversation_headers() {
    let raw = MessageBuilder::new()
        .from(("Aria", "aria@example.com"))
        .to(vec![("", "mara@example.com")])
        .subject("Re: Q2 Report")
        .header("Message-ID", Raw::new("<reply@example.com>"))
        .header("In-Reply-To", Raw::new("<root@example.com>"))
        .header(
            "References",
            Raw::new("<root@example.com> <middle@example.com>"),
        )
        .text_body("Looks good.")
        .write_to_vec()
        .unwrap();
    let mail = parse_email(&raw, 22, "acc1", "INBOX", true, false, &[]).unwrap();
    assert_eq!(mail.meta.message_id.as_deref(), Some("reply@example.com"));
    assert_eq!(mail.meta.thread_id, "root@example.com");
}

#[test]
fn e2e_tampered_mail() {
    let id = test_identity();
    let body = "The amount is 100 USD.";
    let raw = build_raw(
        "Mara Castellanos",
        "mara@aragon.eth",
        "Invoice",
        body,
        Some(&id),
    );
    // 模拟传输中篡改正文（保持长度避免破坏 MIME 结构）
    let tampered = String::from_utf8(raw.clone())
        .unwrap()
        .replace("The amount is 100 USD.", "The amount is 999 USD.");
    let trusted = trusted_for(&id, "Mara Castellanos", "mara@aragon.eth");
    let mail = parse_email(
        tampered.as_bytes(),
        3,
        "acc1",
        "INBOX",
        true,
        false,
        &trusted,
    )
    .unwrap();
    assert_eq!(mail.meta.trust, "tampered");
    match mail.verify {
        VerifyDetail::Tampered {
            signed_hash,
            got_hash,
            ..
        } => assert_ne!(signed_hash, got_hash),
        other => panic!("应为 Tampered，实际 {:?}", other),
    }
}

#[test]
fn e2e_impersonation_by_display_name() {
    let id = test_identity();
    // 未签名，但显示名与可信联系人一致、域名不同 → 冒充
    let raw = build_raw(
        "Mara Castellanos",
        "mara@aragon-finance.io",
        "Urgent payment",
        "Send funds today.",
        None,
    );
    let trusted = trusted_for(&id, "Mara Castellanos", "mara@aragon.eth");
    let mail = parse_email(&raw, 4, "acc1", "INBOX", true, false, &trusted).unwrap();
    assert_eq!(mail.meta.trust, "impersonation");
    match mail.verify {
        VerifyDetail::Impersonation {
            got_domain,
            real_domain,
            ..
        } => {
            assert_eq!(got_domain, "aragon-finance.io");
            assert_eq!(real_domain, "aragon.eth");
        }
        other => panic!("应为 Impersonation，实际 {:?}", other),
    }
}

#[test]
fn e2e_impersonation_wrong_key_same_address() {
    let id_real = test_identity();
    let id_fake = Identity {
        signing_key: ed25519_dalek::SigningKey::from_bytes(&[9u8; 32]),
        created: String::new(),
    };
    // 地址与可信联系人相同，但用了另一把密钥签名 → 冒充/密钥被换
    let raw = build_raw(
        "Mara Castellanos",
        "mara@aragon.eth",
        "Key swap",
        "Trust me.",
        Some(&id_fake),
    );
    let trusted = trusted_for(&id_real, "Mara Castellanos", "mara@aragon.eth");
    let mail = parse_email(&raw, 5, "acc1", "INBOX", true, false, &trusted).unwrap();
    assert_eq!(mail.meta.trust, "impersonation");
}

#[test]
fn e2e_unsigned_mail() {
    let raw = build_raw(
        "Yuki Tanaka",
        "yuki@kanso.jp",
        "こんにちは",
        "初めてご連絡いたします。",
        None,
    );
    let mail = parse_email(&raw, 6, "acc1", "INBOX", false, false, &[]).unwrap();
    assert_eq!(mail.meta.trust, "unsigned");
    assert_eq!(mail.meta.lang, "JA");
}

/// 用 k256 软件密钥模拟 Ledger（同为 EIP-191 personal_sign），构造带
/// eth-personal 签名头的邮件
fn build_raw_eth(
    from_name: &str,
    from_addr: &str,
    subject: &str,
    body: &str,
    secret: &[u8; 32],
    address: &str,
) -> Vec<u8> {
    let signed = crypto::sign_email_eth(address, from_addr, body, |msg| {
        crypto::eth_personal_sign_with_key(secret, msg)
    })
    .unwrap();
    let mut b = MessageBuilder::new()
        .from((from_name, from_addr))
        .to(vec![("", "aria@example.com")])
        .subject(subject)
        .text_body(body);
    for (name, value) in signed.headers {
        b = b.header(name, Raw::new(value));
    }
    b.write_to_vec().unwrap()
}

fn eth_address_of(secret: &[u8; 32]) -> String {
    // 通过一次签名 + 恢复拿到地址（与验证路径同一套实现）
    let sig = crypto::eth_personal_sign_with_key(secret, b"probe").unwrap();
    crypto::eth_personal_recover(b"probe", &sig).unwrap()
}

#[test]
fn eth_personal_sign_recover_roundtrip() {
    let secret = [3u8; 32];
    let msg = b"sealmail-v1|a@b.c|2026-06-11T00:00:00Z|deadbeef";
    let sig = crypto::eth_personal_sign_with_key(&secret, msg).unwrap();
    let addr = crypto::eth_personal_recover(msg, &sig).unwrap();
    assert!(addr.starts_with("0x") && addr.len() == 42);
    // 同一密钥对另一条消息恢复出同一地址
    let sig2 = crypto::eth_personal_sign_with_key(&secret, b"other message").unwrap();
    assert_eq!(
        crypto::eth_personal_recover(b"other message", &sig2).unwrap(),
        addr
    );
    // 消息被篡改 → 恢复出的地址不同
    let addr_tampered = crypto::eth_personal_recover(b"tampered!", &sig).unwrap();
    assert_ne!(addr_tampered, addr);
}

#[test]
fn e2e_eth_verified_mail() {
    let secret = [5u8; 32];
    let address = eth_address_of(&secret);
    let body = "Payload hash attached for co-signing.";
    let raw = build_raw_eth(
        "Mara Castellanos",
        "mara@aragon.eth",
        "Rotation",
        body,
        &secret,
        &address,
    );

    // 可信记录里登记的是 0x 地址
    let trusted = vec![TrustedContact {
        name: "Mara Castellanos".into(),
        email: "mara@aragon.eth".into(),
        fingerprint: address.clone(),
        org: None,
        since: "2025-01-01".into(),
        verified_count: 7,
    }];
    let mail = parse_email(&raw, 10, "acc1", "INBOX", true, false, &trusted).unwrap();
    assert_eq!(mail.meta.trust, "verified");
    match mail.verify {
        VerifyDetail::Verified {
            method,
            fingerprint,
            ..
        } => {
            assert_eq!(method, "Ledger · secp256k1");
            assert!(fingerprint.eq_ignore_ascii_case(&address));
        }
        other => panic!("应为 Verified，实际 {:?}", other),
    }

    // 无可信记录 → signedUnknown
    let mail2 = parse_email(&raw, 11, "acc1", "INBOX", true, false, &[]).unwrap();
    assert_eq!(mail2.meta.trust, "signedUnknown");
}

#[test]
fn e2e_eth_tampered_mail() {
    let secret = [5u8; 32];
    let address = eth_address_of(&secret);
    let body = "Wire 100 USD to account A.";
    let raw = build_raw_eth("Mara", "mara@aragon.eth", "Wire", body, &secret, &address);
    let tampered = String::from_utf8(raw)
        .unwrap()
        .replace("Wire 100 USD to account A.", "Wire 999 USD to account B.");
    let mail = parse_email(tampered.as_bytes(), 12, "acc1", "INBOX", true, false, &[]).unwrap();
    assert_eq!(mail.meta.trust, "tampered");
}

#[test]
fn eth_sign_rejects_wrong_address_binding() {
    // 设备返回的签名恢复出的地址与绑定地址不一致时必须报错（自检）
    let secret = [5u8; 32];
    let err = crypto::sign_email_eth(
        "0x0000000000000000000000000000000000000001",
        "a@b.c",
        "hi",
        |msg| crypto::eth_personal_sign_with_key(&secret, msg),
    );
    assert!(err.is_err());
}

#[test]
fn risk_detection() {
    // 资金 + 紧急 → fund
    let r = detect_risk(
        "Approve transfer",
        "Please wire 250,000 USDC before end of day.",
    )
    .unwrap();
    assert_eq!(r.kind, "fund");
    // 索取助记词 → account（无需紧急词）
    let r = detect_risk(
        "Security check",
        "Please confirm your seed phrase to keep access.",
    )
    .unwrap();
    assert_eq!(r.kind, "account");
    // 合同 + 时限 → contract
    let r = detect_risk("MSA", "Please counter-sign the agreement immediately.").unwrap();
    assert_eq!(r.kind, "contract");
    // 普通邮件 → 无风险
    assert!(detect_risk("Lunch", "Want to grab lunch tomorrow?").is_none());
    // 资金但不紧急 → 不触发
    assert!(detect_risk(
        "Invoice archive",
        "Attached last year's payment records for bookkeeping."
    )
    .is_none());
}

#[test]
fn lang_detection() {
    assert_eq!(detect_lang("Hello world"), "EN");
    assert_eq!(detect_lang("初めてご連絡いたします"), "JA");
    assert_eq!(detect_lang("你好，合作愉快"), "ZH");
}

fn mk_mail(from_addr: &str, subject: &str, body: &str) -> EmailFull {
    let raw = build_raw("Someone", from_addr, subject, body, None);
    parse_email(&raw, 1, "acc1", "INBOX", true, false, &[]).unwrap()
}

#[test]
fn filter_rules_match() {
    let mail = mk_mail(
        "billing@github.com",
        "Your receipt #1234",
        "Thanks for your purchase.",
    );
    let mut rule = FilterRule {
        id: "r1".into(),
        name: "GitHub".into(),
        account_id: None,
        field: "from".into(),
        op: "contains".into(),
        value: "github.com".into(),
        target_folder: "通知".into(),
        mark_read: false,
        enabled: true,
    };
    assert!(rule_matches(&rule, &mail));

    rule.value = "gitlab.com".into();
    assert!(!rule_matches(&rule, &mail));

    rule.field = "subject".into();
    rule.op = "starts_with".into();
    rule.value = "your receipt".into();
    assert!(rule_matches(&rule, &mail), "大小写不敏感的 starts_with");

    rule.enabled = false;
    assert!(!rule_matches(&rule, &mail), "停用的规则不匹配");

    rule.enabled = true;
    rule.account_id = Some("other-acc".into());
    assert!(!rule_matches(&rule, &mail), "限定其他账户的规则不匹配");

    rule.account_id = Some("acc1".into());
    assert!(rule_matches(&rule, &mail));
}

#[test]
fn filter_from_equals_matches_address() {
    // 「发件人 等于 地址」必须能命中：haystack 是 "显示名 地址" 拼接，
    // 不能用全串 equals 比较（bug：jenkins@wanchain.org 规则匹配不到任何邮件）
    let mail = mk_mail(
        "jenkins@wanchain.org",
        "Alert-main-bridge_api",
        "creat_tx2 status",
    );
    let mut rule = FilterRule {
        id: "r2".into(),
        name: "bot".into(),
        account_id: None,
        field: "from".into(),
        op: "equals".into(),
        value: "jenkins@wanchain.org".into(),
        target_folder: "机器人".into(),
        mark_read: true,
        enabled: true,
    };
    assert!(rule_matches(&rule, &mail), "发件人地址精确相等必须命中");

    // 大小写不敏感
    rule.value = "Jenkins@Wanchain.org".into();
    assert!(rule_matches(&rule, &mail));

    // 显示名精确相等也应命中（build_raw 里显示名是 "Someone"）
    rule.value = "someone".into();
    assert!(rule_matches(&rule, &mail), "显示名精确相等必须命中");

    // 不同地址不命中
    rule.value = "other@wanchain.org".into();
    assert!(!rule_matches(&rule, &mail));

    // 地址的子串不算 equals
    rule.value = "wanchain.org".into();
    assert!(!rule_matches(&rule, &mail), "equals 不应做子串匹配");
}

fn mk_rule(id: &str, value: &str, target: &str, mark_read: bool) -> FilterRule {
    FilterRule {
        id: id.into(),
        name: id.into(),
        account_id: None,
        field: "from".into(),
        op: "contains".into(),
        value: value.into(),
        target_folder: target.into(),
        mark_read,
        enabled: true,
    }
}

#[test]
fn plan_moves_groups_by_target_and_respects_rule_order() {
    use sealmail_lib::filters::plan_moves;
    let mut m1 = mk_mail("jenkins@wanchain.org", "Alert 1", "x");
    m1.meta.uid = 11;
    let mut m2 = mk_mail("jenkins@wanchain.org", "Alert 2", "x");
    m2.meta.uid = 12;
    let mut m3 = mk_mail("support@vultr.com", "Notice", "x");
    m3.meta.uid = 13;
    let mut m4 = mk_mail("friend@example.com", "Hi", "x");
    m4.meta.uid = 14;
    let mails = [m1, m2, m3, m4];

    // 两条规则都能匹配 jenkins 时，第一条生效（规则按顺序匹配）
    let rules = vec![
        mk_rule("r1", "jenkins", "机器人", true),
        mk_rule("r2", "wanchain", "商务", false),
        mk_rule("r3", "vultr", "屏蔽", true),
    ];
    let plans = plan_moves(&rules, "acc1", "INBOX", &mails);
    assert_eq!(plans.len(), 2, "jenkins×2 归一组，vultr 归一组，无关邮件不动");
    assert_eq!(plans[0].target, "机器人");
    assert_eq!(plans[0].uids, vec![11, 12], "同目标合并为一组批量移动");
    assert!(plans[0].mark_read);
    assert_eq!(plans[1].target, "屏蔽");
    assert_eq!(plans[1].uids, vec![13]);

    // 目标目录等于来源目录时跳过（避免原地移动）
    let rules = vec![mk_rule("r1", "jenkins", "INBOX", false)];
    assert!(plan_moves(&rules, "acc1", "INBOX", &mails).is_empty());

    // 限定其他账户的规则不参与
    let mut scoped = mk_rule("r1", "jenkins", "机器人", false);
    scoped.account_id = Some("other".into());
    assert!(plan_moves(&[scoped], "acc1", "INBOX", &mails).is_empty());
}

/// 自己（本机身份）签发的邮件，经 trusted_for_verify 注入本人身份后应直接「已验证」，
/// 而不是黄色「签名有效·尚未列入可信」
#[test]
fn e2e_self_signed_mail_is_verified() {
    let dir = std::env::temp_dir().join(format!("sealmail-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = sealmail_lib::store::StoreData::load(dir.clone()).unwrap();
    let account = Account {
        id: "a1".into(),
        label: "Test".into(),
        email: "me@example.com".into(),
        display_name: "Molin".into(),
        protocol: IncomingProtocol::Imap,
        incoming_host: "x".into(),
        incoming_port: 993,
        smtp_host: "x".into(),
        smtp_port: 587,
        smtp_security: "starttls".into(),
        username: "me@example.com".into(),
        auth: "password".into(),
    };

    // 用本机身份给自己发信
    let raw = build_raw(
        "Molin",
        "me@example.com",
        "test",
        "self test\r\n",
        Some(&store.identity),
    );

    // 不注入本人身份：黄色 signedUnknown
    let plain = parse_email(&raw, 1, "a1", "INBOX", false, false, &store.trusted).unwrap();
    assert_eq!(plain.meta.trust, "signedUnknown");

    // 注入本人身份：绿色 verified
    let trusted = store.trusted_for_verify(&account);
    let own = parse_email(&raw, 1, "a1", "INBOX", false, false, &trusted).unwrap();
    assert_eq!(own.meta.trust, "verified");
    match own.verify {
        VerifyDetail::Verified {
            contact_name,
            fingerprint,
            ..
        } => {
            assert_eq!(contact_name, "Molin（本人）");
            assert_eq!(fingerprint, store.identity.fingerprint());
        }
        other => panic!("expected Verified, got {:?}", other),
    }
    let _ = std::fs::remove_dir_all(&dir);
}
