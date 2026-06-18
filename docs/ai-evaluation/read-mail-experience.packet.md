# AI Evaluation Packet: Read Mail Experience

Packet ID: `read-mail-experience`

Scenario ID: `S03`

Primary question:

> SealMail 的阅读体验，是否让正文、发件人、主题、时间、附件这些邮件基本信息成为主角，同时让可信状态以合适层级辅助判断？

## 1. Product Philosophy Source

Evaluator must read and apply:

- `docs/product-philosophy.md`
- `docs/ai-testing-methodology.md`
- `docs/user-scenarios.md`
- `docs/ai-evaluation/rubric.md`

Important principles to apply:

- 阅读邮件是邮件客户端的核心任务。
- 正文阅读空间、发件人识别、附件操作必须优先于特色说明。
- 可信状态要帮助用户理解风险，但不能默认压过正文。
- 高风险邮件需要明确提醒，普通邮件不应被过度审计化。

## 2. User Persona

普通办公用户。

用户打开一封邮件，是为了阅读内容、判断是否需要回复、保存附件或继续处理邮件。

用户不想先理解签名算法、fingerprint、Core、CLI 或风险模型。

## 3. Scenario

用户从收件箱打开一封普通邮件。

Path:

1. 在邮件列表中选择一封邮件。
2. 看到发件人、主题、时间和正文。
3. 查看可信状态。
4. 如果有附件，能看到并保存。
5. 根据内容继续回复、归档、删除或返回列表。

Failure path:

1. 邮件正文解析失败、附件保存失败或远端状态已变化。
2. 用户知道当前失败影响什么。
3. 用户知道是否可以重试、返回列表或继续阅读缓存内容。

## 4. Product Promise

SealMail must provide:

- 正文阅读舒适。
- 发件人、主题和时间清楚。
- 附件存在感足够且可操作。
- 可信状态有层级：可信安静、未知温和、高风险明确。
- 失败状态不误导用户。

## 5. Evidence Inputs

### Fact Layer Evidence

Required:

- CLI `read` result with message body, sender, subject, time, trust, risk, and attachments.
- CLI `thread` result if conversation context is under review.
- CLI `attachment save` result if attachment behavior is under review.
- Error output if parsing or attachment failure path is under review.

Suggested shape:

```json
{
  "readResult": {
    "subject": "Project update",
    "fromAddr": "person@example.com",
    "bodyTextLength": 1200,
    "trust": "unsigned",
    "risk": "none",
    "attachments": [
      { "filename": "plan.pdf", "size": 20480 }
    ]
  },
  "threadResult": {
    "messageCount": 3
  }
}
```

### Experience Layer Evidence

Required for full L5 review:

- Reading screen screenshot.
- Attachment area screenshot or visible text if attachments exist.
- High-risk or unknown-trust screenshot if that state is under review.
- DOM visible text or accessibility snapshot.

Suggested observations:

- Does the body dominate the reading area?
- Can the user identify sender, subject, and time without effort?
- Are trust/risk indicators understandable without technical knowledge?
- Are reply/archive/delete/save attachment actions discoverable?

## 6. Rubric Focus

The evaluator must pay special attention to:

1. Core task clarity:
   - Is reading the message obviously the primary task?
   - Are sender, subject, body, time, and attachments easy to find?

2. Feature restraint:
   - Does security status support reading instead of taking over?
   - Are technical details hidden until useful?

3. State honesty:
   - If parsing or loading fails, is the limitation clear?
   - Does cached content avoid pretending to be freshly loaded?

4. Error actionability:
   - If attachment save fails, does the user know what happened and what to do?

5. Cognitive load:
   - Can the user understand the trust state without learning cryptographic internals?

## 7. Required Output

Output JSON only.

Must conform to:

`docs/ai-evaluation/schema.json`

Minimum required anti-pattern checks:

- Reading screen looks like a verification report instead of an email.
- Body content is visually secondary to badges or diagnostics.
- Attachment state is hidden or ambiguous.
- High-risk state is too quiet.
- Ordinary unknown/unsigned state is too loud.
- GUI state conflicts with CLI facts.

## 8. Pass/Fail Rules

Set `pass=false` if any of these are true:

- User cannot comfortably read the email body.
- Sender, subject, or attachment state is unclear.
- Trust/risk UI dominates ordinary reading.
- High-risk mail lacks clear warning.
- CLI facts do not prove the selected message can be read.
- Any P0 or P1 finding is present.
