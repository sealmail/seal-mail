# AI Evaluation Packet: Organize Mail

Packet ID: `organize-mail`

Scenario ID: `S07`

Primary question:

> SealMail 的整理邮件体验，是否让用户自然完成移动、归档、删除、已读、星标和文件夹管理，而不会被特色能力打断？

## 1. Product Philosophy Source

Evaluator must read and apply:

- `docs/product-philosophy.md`
- `docs/ai-testing-methodology.md`
- `docs/user-scenarios.md`
- `docs/ai-evaluation/rubric.md`

Important principles to apply:

- 整理邮件是日常使用频率很高的核心能力。
- 删除、归档、移动、标记已读必须清楚且可恢复或有确认。
- 可信状态可以辅助判断，但不能阻碍用户处理收件箱。
- 状态变化必须和 CLI/Core 事实一致。

## 2. User Persona

普通办公用户。

用户每天清理收件箱，把邮件归档、移动到项目文件夹、删除无用邮件、标记待处理邮件。

用户希望操作轻快、反馈明确、出错可恢复。

## 3. Scenario

用户整理一批收件箱邮件。

Path:

1. 打开收件箱。
2. 选择或打开一封邮件。
3. 标记已读或未读。
4. 星标或取消星标。
5. 移动到指定文件夹。
6. 归档一封邮件。
7. 删除一封邮件。
8. 创建或删除一个测试文件夹。
9. 列表和计数反映最新状态。

Failure path:

1. 服务器移动失败、删除失败、文件夹创建失败或同步冲突。
2. 用户知道操作是否成功。
3. 用户知道本地缓存和服务器状态是否一致。

## 4. Product Promise

SealMail must provide:

- 整理动作容易找到。
- 结果反馈明确。
- 高风险或破坏性操作有足够确认。
- 本地缓存和服务器状态不自相矛盾。
- 安全特色不打断普通整理工作流。

## 5. Evidence Inputs

### Fact Layer Evidence

Required:

- CLI `mark` result and follow-up `list` or `read` result.
- CLI `flag` result and follow-up evidence.
- CLI `move` result and target folder readback.
- CLI `archive` result and follow-up evidence.
- CLI `delete` result and follow-up evidence.
- CLI `folder create` / `folder delete` result if folder management is under review.

Suggested shape:

```json
{
  "markResult": {
    "uid": 42,
    "read": true,
    "readbackMatches": true
  },
  "moveResult": {
    "uid": 42,
    "target": "Projects",
    "targetReadbackMatches": true
  },
  "deleteResult": {
    "uid": 43,
    "removedFromInbox": true
  }
}
```

### Experience Layer Evidence

Required for full L5 review:

- Inbox action controls screenshot.
- Message action controls screenshot.
- Move/archive/delete success state screenshot or visible text.
- Failure state screenshot or visible text if failure path is under review.
- DOM visible text or accessibility snapshot.

Suggested observations:

- Are common actions reachable without hunting?
- Are destructive actions clear enough?
- Does the list update after actions?
- Does trust/risk UI help prioritization without blocking organization?

## 6. Rubric Focus

The evaluator must pay special attention to:

1. Core task clarity:
   - Can the user quickly clean up the inbox?
   - Are move/archive/delete/mark/flag actions understandable?

2. Basic reliability:
   - Do CLI facts prove operations and readback?

3. State honesty:
   - Does GUI state match server/cache facts after each action?

4. Error actionability:
   - If an operation fails, does the user know whether the message moved, stayed, or needs retry?

5. Feature restraint:
   - Do safety indicators stay supportive during normal organization?

## 7. Required Output

Output JSON only.

Must conform to:

`docs/ai-evaluation/schema.json`

Minimum required anti-pattern checks:

- Common actions are hidden.
- Delete/archive/move result is ambiguous.
- GUI list state conflicts with CLI readback.
- Safety UI blocks normal organization.
- Destructive action lacks enough clarity.
- Failure path leaves user unsure where the message is.

## 8. Pass/Fail Rules

Set `pass=false` if any of these are true:

- User cannot perform common inbox organization actions.
- Operation result is ambiguous.
- CLI facts do not prove action/readback consistency.
- GUI and CLI disagree about message location or state.
- Any P0 or P1 finding is present.
