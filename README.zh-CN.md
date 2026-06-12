# SealMail 信印 — 可信邮件客户端

[English](README.md) | 中文

一个通用的桌面邮件客户端（Tauri 2 + Vite + React + TypeScript），特色是**邮件签名与可信度验证**：
普通邮件客户端负责"收发邮件"，SealMail 还负责"证明邮件可信"。

UI 按 Claude Design 设计稿（`SealMail.dc.html`）实现：火漆封印隐喻、三栏布局 + 验证面板、
高风险拦截、签名发送流程、身份与密钥管理、发件人可信档案。

## 功能

### 通用邮件客户端（基础必须完好）
- **多账户**：IMAP / POP3 收件 + SMTP 发件（SSL / STARTTLS）
- **服务商预设**：Exchange Online (Office 365，Microsoft OAuth2 设备码登录)、自建 Exchange Server、Gmail、iCloud、QQ、163，以及自定义 IMAP/POP3
- 收件箱 / 统一收件箱 / 服务器目录浏览、已读未读、星标、一键归档、回收站安全删除、回复 / 回复全部 / 转发 / 移动 / 删除、本地搜索、会话线程
- SQLite 本地缓存 + 增量同步，离线阅读、分页加载历史、IMAP IDLE / POP3 轮询新邮件通知
- HTML 正文安全渲染（默认阻止远程图片）、附件收发、本地草稿、撤销发送、收件人补全、常用快捷键
- **自建目录**：IMAP 账户在服务器上创建真实目录；POP3 账户使用本地虚拟目录
- **过滤规则**：按 发件人/收件人/主题/正文 × 包含/等于/开头/结尾 匹配 → 自动移动到目录（可选标已读），可"立即整理收件箱"

### 信任层（特色功能）
- 签名身份二选一：本地生成的 Ed25519 密钥（`identity.key`，0600），或 **Ledger 硬件密钥**（secp256k1，EIP-191 `personal_sign`，USB-HID 直连——每次签名都在设备上确认，私钥永不离开硬件）
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
- 首次使用引导：未配置账户时引导你连接邮箱并设置签名身份（无演示数据）

## 发布

推送 `v*` tag 触发 GitHub Actions 发布流程：构建 macOS dmg（Apple Silicon + Intel）
和 Windows zip + NSIS 安装包并发布到 GitHub Release。配置 `APPLE_*` 仓库 secrets
后自动启用 Apple 签名与公证。每次 push/PR 都会跑 CI（测试 + 类型检查）。

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
  微软已停用基础认证，SealMail 使用 OAuth2/XOAUTH2 设备码登录；管理员仍需为租户/邮箱启用 IMAP 与 SMTP AUTH。
- **自建 Exchange Server**：管理员启用 IMAP/POP3 服务后，填公司服务器地址即可；EWS/Graph 原生协议在路线图中。

## 架构

```
src-tauri/src/
  lib.rs          Tauri 命令层（账户/目录/邮件/发送/过滤/可信联系人）
  models.rs       数据模型（Account、EmailMeta/Full、VerifyDetail、FilterRule…）
  crypto.rs       Ed25519 签名/验证、指纹、正文规范化、X-SealMail-* 头
  db.rs           SQLite 邮件缓存（离线阅读、增量同步、分页）
  mail.rs         MIME 解析(mail-parser)、信任判定、风险/语言检测
  imap_client.rs  IMAP（连接/目录/拉取/移动/已读/删除，MOVE 不支持时回退 COPY+DELETE）
  pop3_client.rs  极简 POP3 over TLS（本地虚拟目录归类）
  smtp_client.rs  SMTP 发送(lettre) + MIME 构建(mail-builder) + 签名头
  ledger.rs       Ledger USB-HID 直连（HID framing + Ethereum app APDU）
  filters.rs      过滤规则匹配引擎
  store.rs        本地持久化（accounts/secrets(0600)/filters/trusted/本地目录）
src/
  App.tsx         主框架（三栏 + 验证面板 + 各弹层）
  trust.ts        信任状态文案/检查项/风险横幅映射
  components/     Seal(封印渲染) Sidebar MailList MessageView VerifyRail
                  ComposeModal AccountModal FiltersModal KeysView
                  Onboarding LedgerBindModal ProfileSlideOver RiskModal
```

## 安全说明

- 账户密码保存在本机应用配置目录 `secrets.json`（权限 600），不进入项目目录，绝不提交 git
- 签名私钥 `identity.key` 同上；签名/验证全部本地完成
- 验证不依赖头像、邮件头装饰或语言——只认密钥指纹

## 路线图

- Gmail OAuth2
- YubiKey 支持
- IMAP 服务器端搜索、自定义签名档、多语言 UI
- EWS / Microsoft Graph 原生 Exchange 协议
