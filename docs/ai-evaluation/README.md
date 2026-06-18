# SealMail AI Evaluation Packets

这个目录存放 AI 参与产品体验测试所需的固定上下文。

目标不是让模型“凭感觉评价界面”，而是让模型拿到足够厚的输入后，按固定产品哲学和 rubric 进行结构化评审。

## 文件说明

- `rubric.md`：AI evaluator 必须遵守的评分规则、严重级别、反模式和输出要求。
- `schema.json`：AI evaluator 输出 JSON 必须满足的 schema。
- `*.packet.md`：具体用户场景的评审包模板。

## 为什么需要 packet

如果只问模型：

> 这个页面是否好用？

模型很容易给出泛泛而谈的建议。

有效的 AI 评审必须包含：

1. 产品哲学原文。
2. AI 测试方法论。
3. 具体用户场景。
4. 用户 persona。
5. 产品承诺。
6. CLI 事实证据。
7. GUI 体验证据。
8. 固定 rubric。
9. 强制 JSON 输出 schema。

这就是 AI Evaluation Packet 的作用。

## 使用流程

1. 选择一个 packet，例如 `send-mail-experience.packet.md`。
2. 校验评审资产：
   - `bun run ai-eval:validate`
3. 运行自动化收集事实证据：
   - `bun run ai-eval:evidence`
   - CLI JSON。
   - 真实邮箱 E2E 结果。
   - 错误输出。
4. 构建完整评审包：
   - `bun run ai-eval:packet -- send-mail-experience.packet.md`
5. 运行 GUI 自动化收集体验证据：
   - 截图。
   - DOM 可见文本。
   - 关键状态序列。
6. 把证据填入 packet。
7. 让 AI evaluator 按 `rubric.md` 评审，并只输出符合 `schema.json` 的 JSON。
8. 校验并存档评审结果，用于回归比较。

## 本地运行 AI evaluator

SealMail 不绑定具体 AI 服务。任何能从 stdin 读取完整 packet、向 stdout 输出 JSON 的命令都可以作为 evaluator。

```bash
SEALMAIL_AI_EVALUATOR_CMD='your-ai-command --json' bun run ai-eval:run -- run send-mail-experience.packet.md
```

输出会存到 `tmp/ai-evaluation/*.result.json`。也可以单独校验已有结果：

```bash
bun run ai-eval:run -- validate-result tmp/ai-evaluation/send-mail-experience.result.json
```

CI 默认只校验 packet、schema 和架构边界，不调用真实 AI 服务；真实 AI 评审适合在本地、发布前检查或带密钥的专用工作流中执行。

## 事实证据

`bun run ai-eval:evidence` 会生成：

```text
tmp/ai-evaluation/fact-evidence.json
```

这份文件只来自源码和文档，不读取真实账号、密码或 `.env.local`。它记录：

- CLI/Core 架构入口是否存在。
- GUI 是否通过 `cli_json` 进入业务能力。
- 普通用户向核心邮件能力是否在 CLI、GUI API、用户场景中都有覆盖。
- 当前 packet、scenario 和 Git 状态。

真实邮箱 E2E、GUI 截图、DOM 可见文本等运行时证据应该继续补充到 packet 的 evidence section 中。

## AI evaluator 角色

AI evaluator 不是单元测试，也不是视觉快照测试。

它的角色更像产品审稿人：

- 判断核心邮件体验是否被保护。
- 判断特色能力是否有分寸。
- 判断错误信息是否可行动。
- 判断证据是否足以支撑“完成”。
- 找出传统断言不容易发现的体验风险。

## 不应该做什么

- 不要只给模型一个截图就让它判断。
- 不要只给模型几句抽象原则。
- 不要让模型自由发挥输出格式。
- 不要把 AI evaluator 的主观判断当成唯一事实。
- 不要用 AI evaluator 替代 CLI/Core 的确定性测试。

## 应该做什么

- CLI/Core 负责事实层。
- GUI 自动化负责体验证据采集。
- AI evaluator 负责根据产品哲学审查体验风险。
- 传统测试和 AI 评审结果一起组成完成证据。
