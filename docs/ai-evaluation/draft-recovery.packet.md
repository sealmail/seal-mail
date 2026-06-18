# AI Evaluation Packet: Draft Recovery

Packet ID: `draft-recovery`

Scenario ID: `S05`

Primary question:

> SealMail 的草稿体验，是否让用户相信“写了一半不会丢”，并且不会把草稿误导成已发送邮件？

## 1. Product Philosophy Source

Evaluator must read and apply:

- `docs/product-philosophy.md`
- `docs/ai-testing-methodology.md`
- `docs/user-scenarios.md`
- `docs/ai-evaluation/rubric.md`

Important principles to apply:

- 草稿是写信体验的一部分，不是附属小功能。
- 邮件客户端必须保护用户正在写的内容。
- 状态文案要诚实：草稿、发送中、已发送不能混淆。
- 签名能力不能干扰草稿保存和恢复。

## 2. User Persona

普通办公用户。

用户写邮件时可能被打断、切换窗口、关闭 App 或稍后继续。

用户只关心内容是否还在、能否继续编辑、是否已经发送。

## 3. Scenario

用户写了一封邮件但没有发送，稍后回来继续。

Path:

1. 进入写信。
2. 填写收件人、主题、正文。
3. 保存草稿或等待自动保存。
4. 离开写信界面或重启 App。
5. 打开草稿。
6. 继续编辑或删除草稿。
7. 发送成功后，草稿状态被合理清理。

Failure path:

1. 保存草稿失败、重新加载失败或发送失败。
2. 用户知道草稿是否仍在。
3. 用户知道下一步是重试、继续编辑还是重新保存。

## 4. Product Promise

SealMail must provide:

- 草稿保存和恢复可靠。
- 草稿状态可见但不打扰写作。
- 发送失败不丢内容。
- 草稿不会被误显示为已发送。
- 用户不需要理解本地 Store 或 CLI 才能信任草稿。

## 5. Evidence Inputs

### Fact Layer Evidence

Required:

- CLI `draft save` result.
- CLI `drafts` result proving the saved draft can be listed.
- Store reload or new process evidence proving the draft persists.
- CLI `draft delete` result if deletion is under review.
- Send failure evidence if recovery after failure is under review.

Suggested shape:

```json
{
  "draftSave": {
    "id": "draft-123",
    "subject": "Follow up"
  },
  "draftsAfterReload": {
    "containsDraft": true,
    "bodyMatches": true,
    "toMatches": true
  },
  "deleteResult": {
    "deleted": true
  }
}
```

### Experience Layer Evidence

Required for full L5 review:

- Compose screen with draft state.
- Draft list or recovery entry screenshot.
- Reload/reopen state evidence.
- Failure state screenshot or visible text if failure path is under review.
- DOM visible text or accessibility snapshot.

Suggested observations:

- Does the user know whether content is saved?
- Does draft recovery feel like normal email behavior?
- Is there any wording that implies the draft was already sent?
- Can the user continue editing without reconstructing context?

## 6. Rubric Focus

The evaluator must pay special attention to:

1. Basic reliability:
   - Does fact evidence prove draft roundtrip and persistence?

2. State honesty:
   - Are draft, sending, sent, and failed states distinct?

3. Error actionability:
   - If saving fails, does the UI tell the user whether content is still local?

4. Cognitive load:
   - Does the user need to understand internal storage to trust recovery?

5. Feature restraint:
   - Does signing remain out of the way until sending?

## 7. Required Output

Output JSON only.

Must conform to:

`docs/ai-evaluation/schema.json`

Minimum required anti-pattern checks:

- Draft state is invisible.
- Draft appears sent when it is not.
- Send failure loses or hides content.
- Recovery requires technical knowledge.
- GUI state conflicts with CLI facts.
- Evidence does not prove persistence.

## 8. Pass/Fail Rules

Set `pass=false` if any of these are true:

- Draft content cannot be recovered.
- User cannot tell draft from sent mail.
- Send failure can lose content without warning.
- Evidence does not prove draft save/list/reload behavior.
- Any P0 or P1 finding is present.
