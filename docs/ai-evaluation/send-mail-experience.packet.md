# AI Evaluation Packet: Send Mail Experience

Packet ID: `send-mail-experience`

Scenario ID: `S04`

Primary question:

> SealMail 的写信和发送体验，是否首先是一个自然可靠的邮件客户端体验；签名和可信能力是否只是增强，而不是打扰？

## 1. Product Philosophy Source

Evaluator must read and apply:

- `docs/product-philosophy.md`
- `docs/ai-testing-methodology.md`
- `docs/user-scenarios.md`
- `docs/ai-evaluation/rubric.md`

Important principles to apply:

- SealMail 首先是邮件客户端，其次才是可信邮件客户端。
- 核心体验是生存线，特色能力是差异化。
- 特色能力应该增强核心体验，而不是喧宾夺主。
- 发送失败不能伪装成成功，草稿和附件不能丢。
- 用户不应该为了发送普通邮件理解签名、hash、fingerprint、Core、CLI 等内部概念。

## 2. User Persona

普通办公用户。

用户每天处理邮件，不是安全专家。

用户知道附件、草稿、收件人、主题、正文这些邮件概念，但不理解也不关心 Core、CLI、签名算法或 fingerprint。

## 3. Scenario

用户要发送一封带附件的普通工作邮件。

Path:

1. 打开 SealMail。
2. 进入写信。
3. 填写收件人、主题、正文。
4. 添加附件。
5. 点击发送。
6. 看到发送中状态。
7. 发送成功后回到邮件上下文。
8. 在已发送中能找到邮件。
9. 邮件带有签名，但签名信息不打扰普通写信。

Failure path:

1. SMTP 登录失败、网络失败或附件读取失败。
2. 用户必须知道邮件没有发出。
3. 草稿和附件状态必须保留或明确说明如何恢复。
4. 错误信息必须说明发生了什么、影响是什么、下一步怎么办。

## 4. Product Promise

SealMail must provide:

- 写信流程自然。
- 附件状态清楚。
- 发送结果明确。
- 失败不丢草稿。
- 签名能力默认增强可信度，但不打扰写信。
- GUI 展示和 CLI 事实结果不矛盾。

## 5. Evidence Inputs

The evaluator should expect these evidence blocks.

If evidence is missing, mark it under `evidenceCoverage.missingEvidence`.

### Fact Layer Evidence

Required:

- CLI command used for send.
- CLI send result JSON.
- CLI read result from sent folder.
- Attachment roundtrip result, if attachment was included.
- Any error output, if failure path is under review.

Suggested shape:

```json
{
  "sendResult": {
    "signed": true,
    "method": "SealMail · Ed25519",
    "fingerprint": "...",
    "shortFingerprint": "...",
    "sentAt": "..."
  },
  "sentFolderReadback": {
    "subjectMatches": true,
    "trust": "verified",
    "attachments": 1
  },
  "attachmentRoundtrip": {
    "saved": true,
    "contentMatches": true
  }
}
```

### Experience Layer Evidence

Required for full L5 review:

- Compose screen screenshot.
- Sending state screenshot or visible text.
- Success state screenshot or visible text.
- Failure state screenshot or visible text, if failure path is under review.
- DOM visible text or accessibility snapshot.

Suggested shape:

```json
{
  "visibleText": [
    "New message",
    "To",
    "Subject",
    "Attach",
    "Send"
  ],
  "screenshots": [
    "compose.png",
    "sending.png",
    "sent.png"
  ]
}
```

## 6. Rubric Focus

The evaluator must pay special attention to:

1. Core task clarity:
   - Is writing and sending the main path?
   - Are recipient, subject, body, attachment, and send action obvious?

2. Feature restraint:
   - Does signing stay quiet unless needed?
   - Does trusted-mail language avoid taking over the compose flow?

3. State honesty:
   - Is sending vs sent vs failed clear?
   - Does the UI avoid implying success before CLI facts confirm it?

4. Error actionability:
   - If send fails, does the user know the mail was not sent?
   - Does the error preserve or explain draft recovery?

5. Cognitive load:
   - Does the user need to understand fingerprint, hash, Core, CLI, or transport details to send a normal email?

## 7. Required Output

Output JSON only.

Must conform to:

`docs/ai-evaluation/schema.json`

Minimum required anti-pattern checks:

- Safety feature overwhelms compose flow.
- Send failure appears successful.
- Draft or attachment loss is not acknowledged.
- Technical jargon blocks ordinary send flow.
- GUI state conflicts with CLI facts.
- Secret or token appears in evidence.

## 8. Pass/Fail Rules

Set `pass=false` if any of these are true:

- User cannot clearly complete the send path.
- Evidence does not prove whether the message was sent.
- Send failure is ambiguous or misleading.
- Draft or attachment loss occurs without recovery.
- Security/signing UI dominates the compose experience.
- Any P0 or P1 finding is present.
