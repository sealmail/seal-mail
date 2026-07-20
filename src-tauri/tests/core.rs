//! 核心逻辑端到端测试：签名 → 构造真实 MIME → 解析 → 验证各信任状态。

use base64::{engine::general_purpose::STANDARD, Engine as _};
use encoding_rs::GB18030;
use mail_builder::headers::raw::Raw;
use mail_builder::MessageBuilder;
use sealmail_lib::crypto::{self, Identity};
use sealmail_lib::filters::rule_matches;
use sealmail_lib::mail::{detect_lang, detect_risk, extract_attachment, parse_email};
use sealmail_lib::models::*;

fn test_identity() -> Identity {
    let seed = [7u8; 32];
    Identity {
        signing_key: ed25519_dalek::SigningKey::from_bytes(&seed),
        created: "2026-01-01T00:00:00Z".into(),
    }
}

fn sign_content<'a>(
    subject: &'a str,
    body: &'a str,
    recipients: &'a [String],
    attachments: &'a [(String, Vec<u8>)],
) -> crypto::SignContent<'a> {
    crypto::SignContent {
        subject,
        body_text: body,
        body_html: None,
        recipients,
        attachments,
    }
}

fn default_recipients() -> Vec<String> {
    vec!["aria@example.com".into()]
}

fn seal_headers_from(signed: &crypto::SignedHeaders) -> crypto::SealHeaders {
    let get = |k: &str| {
        signed
            .headers
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    crypto::SealHeaders {
        pubkey: get(crypto::H_PUBKEY),
        signature: get(crypto::H_SIGNATURE),
        from: get(crypto::H_FROM),
        date: get(crypto::H_DATE),
        body_hash: get(crypto::H_BODY_HASH),
        version: get(crypto::H_VERSION),
        subject_hash: get(crypto::H_SUBJECT_HASH),
        html_hash: get(crypto::H_HTML_HASH),
        attach_hash: get(crypto::H_ATTACH_HASH),
        to_hash: get(crypto::H_TO_HASH),
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
    let recipients = default_recipients();
    let mut b = MessageBuilder::new()
        .from((from_name, from_addr))
        .to(vec![("", "aria@example.com")])
        .subject(subject)
        .text_body(body);
    if let Some(id) = identity {
        let content = sign_content(subject, body, &recipients, &[]);
        for (name, value) in crypto::sign_email(id, from_addr, &content).headers {
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
    let body_norm = "Hello Aria,\n\nPlease review the doc.";
    let recipients = default_recipients();
    let content = sign_content("Review", body, &recipients, &[]);
    let signed = crypto::sign_email(&id, "mara@example.com", &content);
    let h = seal_headers_from(&signed);
    assert_eq!(h.version, "2");
    let actual = sign_content("Review", body_norm, &recipients, &[]);
    // CRLF→LF 等规范化后应当一致
    let (fpr, body_ok, _, _) = crypto::verify_headers(&h, &actual).unwrap();
    assert!(body_ok, "规范化后的正文哈希必须一致");
    assert_eq!(fpr, id.fingerprint());

    // 正文被篡改 → body_ok = false
    let tampered = sign_content(
        "Review",
        "Hello Aria,\n\nPlease send 5000 USDC now.",
        &recipients,
        &[],
    );
    let (_, tampered_ok, signed_hash, got_hash) = crypto::verify_headers(&h, &tampered).unwrap();
    assert!(!tampered_ok);
    assert_ne!(signed_hash, got_hash);

    // 主题被改 → v2 失败
    let wrong_subject = sign_content("HACKED", body_norm, &recipients, &[]);
    let (_, subject_ok, _, _) = crypto::verify_headers(&h, &wrong_subject).unwrap();
    assert!(!subject_ok);

    // 收件人被改 → v2 失败（防重放给他人）
    let other_rcpt = vec!["victim@evil.test".into()];
    let wrong_to = sign_content("Review", body_norm, &other_rcpt, &[]);
    let (_, to_ok, _, _) = crypto::verify_headers(&h, &wrong_to).unwrap();
    assert!(!to_ok);

    // 签名本身被篡改 → Err
    let mut bad = h.clone();
    bad.from = "attacker@evil.com".into();
    assert!(crypto::verify_headers(&bad, &actual).is_err());
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
fn preview_from_html_only_gb2312_body() {
    // 仅 HTML + gb2312 的邮件：列表 preview 不能是乱码
    let raw = concat!(
        "From: Weijia Zhang <weijia@example.com>\r\n",
        "To: molin@example.com\r\n",
        "Subject: MPC review from Zhongzhong\r\n",
        "Content-Type: multipart/mixed; boundary=\"outer\"\r\n\r\n",
        "--outer\r\n",
        "Content-Type: text/html; charset=\"gb2312\"\r\n",
        "Content-Transfer-Encoding: quoted-printable\r\n\r\n",
        "<html><body><div>=B1=B3=BE=B0=A3=BAThorchain=A1=A2Zcash</div></body></html>\r\n",
        "--outer\r\n",
        "Content-Type: application/octet-stream; name=\"a.bin\"\r\n",
        "Content-Disposition: attachment; filename=\"a.bin\"\r\n",
        "Content-Transfer-Encoding: base64\r\n\r\n",
        "YQ==\r\n",
        "--outer--\r\n",
    )
    .as_bytes();

    let mail = parse_email(raw, 1, "acc1", "INBOX", true, false, &[]).unwrap();
    assert!(
        !mail.meta.preview.contains('\u{FFFD}'),
        "list preview must not be mojibake: {:?}",
        mail.meta.preview
    );
    assert!(
        mail.meta.preview.contains("背景") && mail.meta.preview.contains("Thorchain"),
        "preview should include decoded Chinese: {:?}",
        mail.meta.preview
    );
    assert_eq!(mail.meta.lang, "ZH");
}

#[test]
fn decodes_gb2312_rfc2047_attachment_filename() {
    // 真实 Outlook/Exchange 常见写法：filename="=?gb2312?B?...?="
    // 回归：AI代码审计结果评估.docx（base64+gb2312）
    let raw = concat!(
        "From: Weijia Zhang <weijia@example.com>\r\n",
        "To: molin@example.com\r\n",
        "Subject: MPC review from Zhongzhong\r\n",
        "Content-Type: multipart/mixed; boundary=\"outer\"\r\n\r\n",
        "--outer\r\n",
        "Content-Type: text/html; charset=\"gb2312\"\r\n",
        "Content-Transfer-Encoding: quoted-printable\r\n\r\n",
        // 背景： (gb2312 QP)
        "=B1=B3=BE=B0=A3=BAThorchain\r\n",
        "--outer\r\n",
        "Content-Type: application/vnd.openxmlformats-officedocument.wordprocessingml.document;\r\n",
        "\tname=\"=?gb2312?B?QUm0+sLryfO8xr3hufvGwLnALmRvY3g=?=\"\r\n",
        "Content-Disposition: attachment;\r\n",
        "\tfilename=\"=?gb2312?B?QUm0+sLryfO8xr3hufvGwLnALmRvY3g=?=\"; size=12;\r\n",
        "Content-Transfer-Encoding: base64\r\n\r\n",
        "UEsDBAoAAAAAAA==\r\n",
        "--outer--\r\n",
    )
    .as_bytes();

    let mail = parse_email(raw, 1, "acc1", "INBOX", true, false, &[]).unwrap();
    assert_eq!(mail.attachments.len(), 1);
    assert_eq!(mail.attachments[0].name, "AI代码审计结果评估.docx");
    assert!(
        !mail.attachments[0].name.contains('\u{FFFD}'),
        "filename must not contain replacement chars: {}",
        mail.attachments[0].name
    );

    let extracted = extract_attachment(raw, 0).unwrap();
    assert_eq!(extracted.filename, "AI代码审计结果评估.docx");

    // HTML 正文也应按 charset=gb2312 正确解码（列表预览/纯文本回退依赖此路径）
    assert!(
        mail.body_html
            .as_deref()
            .is_some_and(|h| h.contains("背景") && h.contains("Thorchain")),
        "html body should decode gb2312: {:?}",
        mail.body_html.as_ref().map(|s| s.chars().take(80).collect::<String>())
    );
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
    assert_eq!(mail.attachments[0].name, "screenshot.jpg");
    assert_eq!(mail.attachments[0].mime, "image/jpeg");

    let extracted = extract_attachment(&raw, 0).unwrap();
    assert_eq!(extracted.filename, "screenshot.jpg");
    assert_eq!(extracted.mime, "image/jpeg");
    assert!(!extracted.contents.is_empty());
    // JPEG SOI marker from the fixture base64 payload
    assert_eq!(&extracted.contents[..2], &[0xff, 0xd8]);
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

/// 签名时间在 24h 以后 → 即便密钥可信也不给绿标（防离谱未来戳）。
#[test]
fn future_signature_date_is_not_verified() {
    use ed25519_dalek::Signer;

    let id = test_identity();
    let from = "mara@aragon.eth";
    let body = "Pay invoice 42";
    let subject = "Invoice";
    let recipients = default_recipients();
    let future = (chrono::Utc::now() + chrono::Duration::days(30)).to_rfc3339();
    let content = sign_content(subject, body, &recipients, &[]);
    let hashes = crypto::content_hashes(&content);
    let canon = crypto::canon_string_v2(
        from,
        &future,
        &hashes.subject,
        &hashes.body,
        &hashes.html,
        &hashes.attach,
        &hashes.to,
    );
    let sig = id.signing_key.sign(canon.as_bytes());
    let raw = MessageBuilder::new()
        .from(("Mara Castellanos", from))
        .to(vec![("", "aria@example.com")])
        .subject(subject)
        .text_body(body)
        .header(crypto::H_VERSION, Raw::new("2"))
        .header(crypto::H_METHOD, Raw::new("ed25519"))
        .header(crypto::H_PUBKEY, Raw::new(id.public_key_b64()))
        .header(crypto::H_FROM, Raw::new(from))
        .header(crypto::H_DATE, Raw::new(future))
        .header(crypto::H_SUBJECT_HASH, Raw::new(hashes.subject))
        .header(crypto::H_BODY_HASH, Raw::new(hashes.body))
        .header(crypto::H_HTML_HASH, Raw::new(hashes.html))
        .header(crypto::H_ATTACH_HASH, Raw::new(hashes.attach))
        .header(crypto::H_TO_HASH, Raw::new(hashes.to))
        .header(crypto::H_SIGNATURE, Raw::new(STANDARD.encode(sig.to_bytes())))
        .write_to_vec()
        .unwrap();
    let trusted = trusted_for(&id, "Mara Castellanos", from);
    let mail = parse_email(&raw, 7, "acc1", "INBOX", true, false, &trusted).unwrap();
    assert_eq!(mail.meta.trust, "signedUnknown");
    assert!(matches!(mail.verify, VerifyDetail::SignedUnknown { .. }));
}

/// 攻击者用自签密钥构造含多字节字符的 X-SealMail-Body-Hash。
/// 旧实现对 `&signed_hash[..12]` 按字节切，会在 UTF-8 字符中间 panic。
/// 解析路径必须返回 Tampered，绝不能崩溃。
#[test]
fn malicious_multibyte_body_hash_does_not_panic() {
    use ed25519_dalek::Signer;

    let id = test_identity();
    let from = "attacker@evil.test";
    let body = "looks fine";
    // "aa"(2) + 😀×5：字节下标 12 落在第 3 个 emoji 中间 → 旧切片会 panic
    let evil_hash = format!("aa{}", "😀".repeat(5));
    let date = "2026-01-01T00:00:00Z";
    let canon = crypto::canon_string(from, date, &evil_hash);
    let sig = id.signing_key.sign(canon.as_bytes());
    let sig_b64 = STANDARD.encode(sig.to_bytes());

    let raw = MessageBuilder::new()
        .from(("Attacker", from))
        .to(vec![("", "victim@example.com")])
        .subject("hi")
        .text_body(body)
        .header(crypto::H_VERSION, Raw::new("1"))
        .header(crypto::H_METHOD, Raw::new("ed25519"))
        .header(crypto::H_PUBKEY, Raw::new(id.public_key_b64()))
        .header(crypto::H_FROM, Raw::new(from))
        .header(crypto::H_DATE, Raw::new(date))
        .header(crypto::H_BODY_HASH, Raw::new(evil_hash))
        .header(crypto::H_SIGNATURE, Raw::new(sig_b64))
        .write_to_vec()
        .unwrap();

    let mail = parse_email(&raw, 99, "acc1", "INBOX", true, false, &[]).expect("must not panic");
    assert_eq!(mail.meta.trust, "tampered");
    match mail.verify {
        VerifyDetail::Tampered { signed_hash, got_hash, .. } => {
            assert!(
                signed_hash.ends_with('…'),
                "展示用哈希应安全截断并带省略号: {signed_hash}"
            );
            assert!(got_hash.ends_with('…'), "got_hash 也应安全截断: {got_hash}");
            // 截断后的字符串必须仍是合法 UTF-8（能构造 String 即通过；再断言不含替换符）
            assert!(!signed_hash.contains('\u{FFFD}'));
        }
        other => panic!("应为 Tampered，实际 {:?}", other),
    }
}

/// v1 签名 + 另附 HTML：正文签名有效但 HTML 未覆盖 → 不得给完整绿标 Verified
#[test]
fn v1_signed_with_html_is_not_full_verified() {
    use ed25519_dalek::Signer;

    let id = test_identity();
    let from = "mara@aragon.eth";
    let body = "Pay 100 only.";
    let date = chrono::Utc::now().to_rfc3339();
    let bh = crypto::body_hash_hex(body);
    let canon = crypto::canon_string(from, &date, &bh);
    let sig = id.signing_key.sign(canon.as_bytes());
    let raw = MessageBuilder::new()
        .from(("Mara Castellanos", from))
        .to(vec![("", "aria@example.com")])
        .subject("Pay")
        .text_body(body)
        .html_body("<b>Pay 99999 instead</b>")
        .header(crypto::H_VERSION, Raw::new("1"))
        .header(crypto::H_METHOD, Raw::new("ed25519"))
        .header(crypto::H_PUBKEY, Raw::new(id.public_key_b64()))
        .header(crypto::H_FROM, Raw::new(from))
        .header(crypto::H_DATE, Raw::new(date))
        .header(crypto::H_BODY_HASH, Raw::new(bh))
        .header(crypto::H_SIGNATURE, Raw::new(STANDARD.encode(sig.to_bytes())))
        .write_to_vec()
        .unwrap();
    let trusted = trusted_for(&id, "Mara Castellanos", from);
    let mail = parse_email(&raw, 88, "acc1", "INBOX", true, false, &trusted).unwrap();
    assert_eq!(mail.meta.trust, "signedUnknown");
}

/// v2：附件被替换 → Tampered
#[test]
fn v2_attachment_swap_is_tampered() {
    let id = test_identity();
    let from = "mara@aragon.eth";
    let body = "see attachment";
    let subject = "Invoice PDF";
    let recipients = default_recipients();
    let attach = vec![("invoice.pdf".into(), b"PDF-REAL".to_vec())];
    let content = sign_content(subject, body, &recipients, &attach);
    let signed = crypto::sign_email(&id, from, &content);
    let mut b = MessageBuilder::new()
        .from(("Mara Castellanos", from))
        .to(vec![("", "aria@example.com")])
        .subject(subject)
        .text_body(body)
        .attachment("application/pdf", "invoice.pdf", b"PDF-FAKE".as_slice());
    for (name, value) in signed.headers {
        b = b.header(name, Raw::new(value));
    }
    let raw = b.write_to_vec().unwrap();
    let trusted = trusted_for(&id, "Mara Castellanos", from);
    let mail = parse_email(&raw, 77, "acc1", "INBOX", true, false, &trusted).unwrap();
    assert_eq!(mail.meta.trust, "tampered");
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
    let recipients = default_recipients();
    let content = sign_content(subject, body, &recipients, &[]);
    let signed = crypto::sign_email_eth(address, from_addr, &content, |msg| {
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
    let recipients = default_recipients();
    let content = sign_content("s", "hi", &recipients, &[]);
    let err = crypto::sign_email_eth(
        "0x0000000000000000000000000000000000000001",
        "a@b.c",
        &content,
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
fn would_move_out_detects_blocked_sender_for_notification_suppress() {
    use sealmail_lib::filters::would_move_out;
    let mail = mk_mail("spam@evil.test", "Buy now", "click me");
    let rules = vec![mk_rule("block", "spam@evil.test", "&V4NXPpCuTvY-", true)];
    assert!(
        would_move_out(&rules, "acc1", "INBOX", &mail),
        "屏蔽规则命中时应抑制系统通知"
    );
    assert!(
        !would_move_out(&rules, "acc1", "&V4NXPpCuTvY-", &mail),
        "已经在目标目录则不算会再移出"
    );
    let clean = mk_mail("friend@example.com", "Hi", "hello");
    assert!(!would_move_out(&rules, "acc1", "INBOX", &clean));
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


