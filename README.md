# SealMail 信印 — 可信邮件客户端

一个通用的桌面邮件客户端（Tauri 2 + Vite + React + TypeScript），特色是**邮件签名与可信度验证**：
普通邮件客户端负责"收发邮件"，SealMail 还负责"证明邮件可信"。

UI 按 Claude Design 设计稿（`SealMail.dc.html`）实现：火漆封印隐喻、三栏布局 + 验证面板、
高风险拦截、签名发送流程、身份与密钥管理、发件人可信档案。

## 功能

### 通用邮件客户端（基础必须完好）
- **多账户**：IMAP / POP3 收件 + SMTP 发件（SSL / STARTTLS）
- **服务商预设**：Exchange Online (Office 365)、自建 Exchange Server、Gmail、iCloud、QQ、163，以及自定义 IMAP/POP3
- 收件箱 / 服务器目录浏览、已读未读、回复 / 转发 / 移动 / 删除、本地搜索
- **自建目录**：IMAP 账户在服务器上创建真实目录；POP3 账户使用本地虚拟目录
- **过滤规则**：按 发件人/收件人/主题/正文 × 包含/等于/开头/结尾 匹配 → 自动移动到目录（可选标已读），可"立即整理收件箱"

### 信任层（特色功能）
- 本地生成 Ed25519 身份密钥（`identity.key`，0600，私钥不出本机）
- 发送时可签名：签名信息放在 `X-SealMail-*` 邮件头里，对普通收件人**不可见**；
  正文仅追加一行标准 `-- ` 签名档格式的低调说明，不影响普通邮箱阅读
- 收件时本地验证，五种状态（火漆封印语义）：
  - 🟢 **完整封印** 已验证本人（签名有效 + 指纹与可信记录一致）
  - 🟡 **金色封印** 签名有效 · 尚未列入可信（可一键加入可信联系人）
  - ⚪ **空印环** 未盖印 · 身份未知
  - 🔴 **裂开的封印** 内容被改动（正文哈希与签名不符）
  - 🔴 **伪造封印** 冒充已知联系人（显示名相同但密钥/域名不符）
- 高风险提醒：付款（资金+紧急措辞）、账号安全（索取助记词/密码类）、合同（条款+时限）启发式检测，
  风险弹窗需勾选"已独立核实"才能继续
- 未配置账户时显示设计稿中的 6 封演示邮件（覆盖全部信任状态）

## 运行

```bash
bun install
bun run tauri dev      # 开发
bun run tauri build    # 打包
```

测试：

```bash
cd src-tauri && cargo test   # 签名/验证/篡改/冒充/过滤/风险检测 端到端测试
bunx tsc --noEmit            # 前端类型检查
```

## Exchange 接入说明

- **Exchange Online / Office 365**：预设 `outlook.office365.com:993`（IMAP）+ `smtp.office365.com:587`（STARTTLS）。
  微软已停用基础认证，需要管理员启用 IMAP + SMTP AUTH，个人账户使用应用密码。
  OAuth2（XOAUTH2）设备码流程在路线图中。
- **自建 Exchange Server**：管理员启用 IMAP/POP3 服务后，填公司服务器地址即可；EWS/Graph 原生协议在路线图中。

## 架构

```
src-tauri/src/
  lib.rs          Tauri 命令层（账户/目录/邮件/发送/过滤/可信联系人）
  models.rs       数据模型（Account、EmailMeta/Full、VerifyDetail、FilterRule…）
  crypto.rs       Ed25519 签名/验证、指纹、正文规范化、X-SealMail-* 头
  mail.rs         MIME 解析(mail-parser)、信任判定、风险/语言检测
  imap_client.rs  IMAP（连接/目录/拉取/移动/已读/删除，MOVE 不支持时回退 COPY+DELETE）
  pop3_client.rs  极简 POP3 over TLS（本地虚拟目录归类）
  smtp_client.rs  SMTP 发送(lettre) + MIME 构建(mail-builder) + 签名头
  filters.rs      过滤规则匹配引擎
  store.rs        本地持久化（accounts/secrets(0600)/filters/trusted/本地目录）
src/
  App.tsx         主框架（三栏 + 验证面板 + 各弹层）
  trust.ts        信任状态文案/检查项/风险横幅映射
  demo.ts         设计稿 6 封演示邮件
  components/     Seal(封印渲染) Sidebar MailList MessageView VerifyRail
                  ComposeModal AccountModal FiltersModal KeysView
                  ProfileSlideOver RiskModal
```

## 安全说明

- 账户密码保存在本机应用配置目录 `secrets.json`（权限 600），不进入项目目录，绝不提交 git
- 签名私钥 `identity.key` 同上；签名/验证全部本地完成
- 验证不依赖头像、邮件头装饰或语言——只认密钥指纹

## 路线图

- IMAP IDLE 实时推送、本地邮件缓存（SQLite）
- OAuth2 / XOAUTH2（Exchange Online、Gmail）
- 硬件密钥签名（Ledger / YubiKey）
- 附件下载/发送附件、HTML 正文安全渲染
- EWS / Microsoft Graph 原生 Exchange 协议
