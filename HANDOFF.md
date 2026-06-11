# HANDOFF — SealMail 信印

> 工作交接/进度文档。**每次修改代码后必须同步更新本文件。**
> 最后更新：2026-06-11（v2 进行中）

## 项目定位

通用桌面邮件客户端（Tauri 2 + Vite + React + TS），特色是邮件签名与可信度验证（火漆封印隐喻）。
**基础邮件功能必须完好，签名只是特色。** 这是正式软件，不是 demo。

用户硬性要求（不可违背）：
- IMAP/POP3 连主流邮箱，尤其要支持 Exchange
- 发出的邮件普通邮箱必须能正常收，签名信息不突兀（放 X-SealMail-* 头 + 一行 `-- ` 签名档）
- 过滤规则 + 自建目录
- 不放 demo 数据；首次使用引导用户添加账户
- 标题栏可拖拽移动窗口
- Ledger 硬件签名（参考 ../auto-desktop 的纯 Rust HID 实现）
- 发布用 GitHub Actions（参考 ../auto-desktop/.github/workflows/release.yml）

## 已完成

### v1（初始实现，commit 3e86574 / 0bc3192）
- Rust 后端：`crypto.rs`（Ed25519 签名/验证、指纹、正文规范化、X-SealMail-* 头）、
  `imap_client.rs`（目录/拉取/移动/已读/删除，MOVE 失败回退 COPY+DELETE）、
  `pop3_client.rs`（手写 POP3 over TLS，本地虚拟目录）、
  `smtp_client.rs`（lettre + mail-builder，签名头 + 低调签名档）、
  `mail.rs`（mail-parser 解析、五种信任判定、风险/语言启发式）、
  `filters.rs`、`store.rs`（accounts/secrets(0600)/filters/trusted/local_folders JSON 持久化）
- 信任模型：verified / signedUnknown / unsigned / tampered / impersonation；
  冒充判定 = 显示名匹配可信联系人但密钥指纹或域名不符
- 前端：三栏 + 验证面板 UI 按设计稿实现（设计稿在 /tmp/sealmail_design/，
  原型 SealMail.dc.html）；Seal 封印渲染、风险横幅/弹窗、写邮件三步流程、
  身份与密钥页、发件人档案、过滤规则管理、新建目录、账户向导（Exchange/Gmail/QQ/163/iCloud 预设）
- 测试：`src-tauri/tests/core.rs` 10 个端到端测试（签名往返/篡改/两种冒充/过滤/风险）全过
- README.md 英文 + README.zh-CN.md 中文

### v2（本轮：去 demo 化 + Ledger + 发布流水线）
- [x] HANDOFF.md（本文件）
- [x] 移除 demo 数据（删 demo.ts、api.ts 演示分支、App 演示账户/横幅）
- [x] 首次使用引导：无账户时显示欢迎页（添加账户 CTA + 签名身份说明），不再显示假邮件
- [x] 标题栏拖拽：capabilities 显式加 `core:window:allow-start-dragging`，
      titlebar 空白区域 data-tauri-drag-region（按钮/输入框不受影响）
- [x] Ledger 硬件签名（参考 auto-desktop ledger.rs 适配）：
      - `ledger.rs`：HID framing + ETH app APDU（GET_ADDRESS / SIGN_PERSONAL），hidapi
      - 签名方案二选一：本地 Ed25519 或 Ledger secp256k1（EIP-191 personal_sign）
      - 验证：k256 ecrecover + keccak256，恢复地址比对 X-SealMail-Address
      - 头扩展：X-SealMail-Method (ed25519|eth-personal)、X-SealMail-Address
      - 身份配置持久化 identity.json（mode/path/address）
      - KeysView：绑定 Ledger（取 0-4 号地址选择）/ 切回本地密钥
      - Compose：Ledger 模式下发送前真实等待设备确认
- [x] GitHub Actions：
      - `release.yml`：v* tag 触发，macOS (aarch64+x64 dmg) + Windows (zip+NSIS)，
        无 Apple 证书时跳过签名公证（secrets 存在才签名），staged 资产 → GitHub Release
      - `ci.yml`：push/PR 跑 cargo test + tsc + vite build
- [x] 测试补充：eth-personal 验证往返（k256 本地模拟 Ledger 签名 → ecrecover 验证）

## 待办 / 路线图（按优先级）

1. **OAuth2 / XOAUTH2**（Exchange Online、Gmail 个人账户基础认证均被淘汰，应用密码是过渡方案）
   - IMAP XOAUTH2 SASL + SMTP AUTH XOAUTH2；设备码流程 UI
2. **本地邮件缓存**（SQLite）：目前每次刷新全量拉最近 30 封 BODY.PEEK[]，应增量缓存
3. IMAP IDLE 实时推送 / 定时轮询
4. 附件：下载保存、发送带附件
5. HTML 正文安全渲染（目前只渲染纯文本部分；HTML 已解析但未展示）
6. 多语言 UI（目前中文）
7. 删除确认 / 撤销；草稿箱
8. macOS 公证（需 Apple Developer 证书，secrets 同 auto-desktop：
   APPLE_CERTIFICATE / APPLE_CERTIFICATE_PASSWORD / APPLE_ID / APPLE_PASSWORD / APPLE_TEAM_ID）

## 关键架构决策（勿推翻）

- 签名 canon：`sealmail-v1|from(小写)|date(RFC3339)|sha256(规范化正文)`；
  规范化 = CRLF→LF、去行尾空白、去末尾空行。签名档（`-- ` 行）在签名**之前**追加，包含在哈希内
- 密码/私钥只存本机应用配置目录（macOS: `~/Library/Application Support/com.sealmail.app/`），
  secrets.json 与 identity.key 权限 0600，绝不进 git
- POP3 无服务器目录 → local_assign.json 本地虚拟目录；IMAP 目录是服务器真实目录
- 验证只认密钥指纹/地址，不看头像、显示名、语言
- Ledger 通信走 Rust hidapi（WKWebView 无 WebHID），协议层（framing/APDU/解析）可单测，
  设备 I/O 需要实机

## 构建 / 测试 / 发布

```bash
bun install
bun run tauri dev                 # 开发
bun run tauri build               # 本机打包
cd src-tauri && cargo test        # 后端测试
bunx tsc --noEmit                 # 前端类型检查
git tag v0.x.y && git push --tags # 触发 release workflow
```

## 已知问题 / 注意事项

- imap crate 2.4.1 有 future-incompat 警告（imap-proto 0.10），后续升级 imap 3.x alpha 时一并处理
- IMAP 仅支持 993 隐式 TLS（imap 2.4 无 STARTTLS）；SMTP 支持 SSL/STARTTLS
- 风险检测是关键词启发式（mail.rs 中 FUND_KW/ACCOUNT_KW/CONTRACT_KW/URGENT_KW），误报漏报可调
- Ledger 签名用 ETH app 的 personal_sign（设备屏幕显示的是 canon 字符串哈希），
  需要设备解锁并打开 Ethereum app
