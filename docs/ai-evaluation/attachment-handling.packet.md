# AI Evaluation Packet: Attachment Handling

Packet ID: `attachment-handling`

Scenario ID: `S06`

Primary question:

> SealMail 的附件体验，是否让用户清楚知道附件已经添加、发送、收到或保存，并且失败时不会静默损坏或丢失？

## 1. Product Philosophy Source

Evaluator must read and apply:

- `docs/product-philosophy.md`
- `docs/ai-testing-methodology.md`
- `docs/user-scenarios.md`
- `docs/ai-evaluation/rubric.md`

Important principles to apply:

- 附件是日常邮件工作流的核心能力。
- 附件状态必须清楚，不能只作为技术细节。
- 安全能力不能遮挡或弱化附件操作。
- 内容完整性必须由事实证据证明。

## 2. User Persona

普通办公用户。

用户经常发送合同、图片、表格、PDF 或其他工作文件。

用户最关心：文件有没有带上、对方能不能收到、自己能不能保存、内容有没有损坏。

## 3. Scenario

用户发送并接收一封带附件的邮件。

Path:

1. 在写信界面添加附件。
2. 看到附件名称、大小和可移除状态。
3. 发送邮件。
4. 在已发送或收件箱读回邮件。
5. 查看附件列表。
6. 保存附件。
7. 比对保存后的内容。

Failure path:

1. 附件路径无效、读取失败、发送失败或保存失败。
2. 用户知道邮件是否已发送。
3. 用户知道附件是否包含在邮件中。
4. 用户知道如何修复或重试。

## 4. Product Promise

SealMail must provide:

- 添加附件状态明确。
- 发送结果能证明附件被处理。
- 读信时附件清楚可见。
- 保存附件失败可理解。
- 附件内容不能静默损坏。

## 5. Evidence Inputs

### Fact Layer Evidence

Required:

- CLI `send --attach` command and result.
- CLI `read` result showing attachment metadata.
- CLI `attachment save` result.
- Hash or byte comparison proving saved content matches original.
- Error output if failure path is under review.

Suggested shape:

```json
{
  "sendWithAttachment": {
    "sent": true,
    "attachmentCount": 1
  },
  "readResult": {
    "attachments": [
      { "filename": "contract.pdf", "size": 20480 }
    ]
  },
  "saveResult": {
    "saved": true,
    "sha256Matches": true
  }
}
```

### Experience Layer Evidence

Required for full L5 review:

- Compose attachment area screenshot.
- Sending/sent state screenshot or visible text.
- Read view attachment area screenshot.
- Save success or failure screenshot.
- DOM visible text or accessibility snapshot.

Suggested observations:

- Can the user identify attached files before sending?
- Can the user remove or retry an attachment?
- Does the read view make attachments easy to save?
- Does any security UI obscure the attachment task?

## 6. Rubric Focus

The evaluator must pay special attention to:

1. Basic reliability:
   - Does evidence prove attachment send/read/save roundtrip?
   - Does evidence prove content integrity?

2. Core task clarity:
   - Are add, remove, view, save operations discoverable?

3. State honesty:
   - Does the UI distinguish attached, uploading/sending, sent, saved, and failed?

4. Error actionability:
   - If attachment handling fails, does the user know what to fix?

5. Feature restraint:
   - Does trust/signature UI avoid covering attachment state?

## 7. Required Output

Output JSON only.

Must conform to:

`docs/ai-evaluation/schema.json`

Minimum required anti-pattern checks:

- Attachment presence is ambiguous.
- Attachment failure is hidden behind generic send failure.
- Content integrity is not proven.
- Security UI obscures attachment operations.
- GUI state conflicts with CLI facts.

## 8. Pass/Fail Rules

Set `pass=false` if any of these are true:

- User cannot tell whether an attachment is included.
- Attachment save result is ambiguous.
- Evidence does not prove attachment content integrity.
- Attachment failure can be mistaken for success.
- Any P0 or P1 finding is present.
