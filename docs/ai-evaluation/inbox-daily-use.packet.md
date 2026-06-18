# AI Evaluation Packet: Inbox Daily Use

Packet ID: `inbox-daily-use`

Scenario ID: `S02`

Primary question:

> SealMail 的首屏和收件箱体验，是否首先帮助用户处理日常邮件，而不是把安全特色放在邮件体验前面？

## 1. Product Philosophy Source

Evaluator must read and apply:

- `docs/product-philosophy.md`
- `docs/ai-testing-methodology.md`
- `docs/user-scenarios.md`
- `docs/ai-evaluation/rubric.md`

Important principles to apply:

- 邮件列表、目录、阅读入口是邮件客户端的基本盘。
- 可信状态应该帮助判断，但不能成为每封邮件的主视觉。
- 同步状态必须诚实。
- 用户应该快速进入“处理邮件”的状态。

## 2. User Persona

普通办公用户。

用户打开 App 是为了看有没有新邮件、快速处理收件箱。

用户不是为了进入安全审计后台，也不是为了阅读验证报告。

## 3. Scenario

用户每天早上打开 SealMail 查看收件箱。

Path:

1. 打开 App。
2. 看到当前账号和文件夹。
3. 看到邮件列表。
4. 触发或等待同步。
5. 打开一封普通邮件。
6. 根据需要标记、归档、删除或回复。

Failure path:

1. 网络失败或邮箱服务暂时不可用。
2. 用户知道当前看到的是缓存还是最新状态。
3. 用户知道是否需要重试。

## 4. Product Promise

SealMail must provide:

- 邮件列表是主角。
- 当前账号、当前目录、同步状态清楚。
- 可信状态作为辅助信息存在。
- 安全提示只有在高风险时才强突出。
- 缓存状态和同步失败不误导用户。

## 5. Evidence Inputs

### Fact Layer Evidence

Required:

- CLI `folders` result.
- CLI `sync` result for INBOX.
- CLI `list` result with metas and total.
- Optional CLI `read` result for selected message.

Suggested shape:

```json
{
  "folders": [
    { "name": "INBOX", "display": "INBOX" }
  ],
  "syncResult": {
    "added": 2,
    "total": 120
  },
  "listResult": {
    "total": 120,
    "metas": [
      {
        "subject": "Project update",
        "fromAddr": "person@example.com",
        "trust": "unsigned",
        "unread": true
      }
    ]
  }
}
```

### Experience Layer Evidence

Required for full L5 review:

- Initial inbox screenshot.
- Syncing state screenshot or visible text.
- Synced state screenshot or visible text.
- Error state screenshot or visible text if failure path is under review.
- DOM visible text or accessibility snapshot.

Suggested observations:

- Which area dominates the first screen?
- Are account, folder, and list visually discoverable?
- Are security badges secondary for normal mail?
- Does sync feedback explain freshness without causing alarm?

## 6. Rubric Focus

The evaluator must pay special attention to:

1. Core task clarity:
   - Can the user immediately recognize this as a usable email client?
   - Is the email list more prominent than safety explanations?

2. Basic reliability:
   - Does fact evidence prove folders, sync, and list are working?
   - Does the interface distinguish cache from fresh sync?

3. Feature restraint:
   - Are trust/risk indicators restrained on normal mail?
   - Are high-risk states allowed to become prominent only when necessary?

4. State honesty:
   - Is sync progress or failure clear?
   - Does the product avoid pretending the list is fresh when sync failed?

5. Cognitive load:
   - Can a user process email without learning signature internals?

## 7. Required Output

Output JSON only.

Must conform to:

`docs/ai-evaluation/schema.json`

Minimum required anti-pattern checks:

- Inbox looks like a security dashboard instead of an email client.
- Security labels dominate ordinary messages.
- Sync failure is hidden or ambiguous.
- User cannot identify current account/folder.
- GUI state conflicts with CLI facts.
- Technical terminology blocks ordinary inbox use.

## 8. Pass/Fail Rules

Set `pass=false` if any of these are true:

- User cannot identify the current inbox or message list.
- Sync/cache state is misleading.
- Security UI dominates normal inbox use.
- CLI facts do not prove that folders/list/sync work.
- Any P0 or P1 finding is present.
