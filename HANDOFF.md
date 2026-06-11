# HANDOFF — SealMail 信印

> 工作交接/进度文档。**每次修改代码后必须同步更新本文件。**
> 最后更新：2026-06-11（v8：阅读窗一键信任发件人）

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

### v3（自动升级，UX 参考 auto-desktop）
- [x] tauri-plugin-updater + tauri-plugin-process（desktop only）；bundle.createUpdaterArtifacts
- [x] 升级 UX（src/updater.ts + KeysView「关于与更新」区块）：
      检查更新 → 发现新版本 → 安装更新（下载进度条/安装中）→ 自动重启；
      插件不可用时回退后端 check_for_update（GitHub API）→「打开下载页面」手动升级
- [x] updater.rs：版本比较 + GitHub Releases 回退检测（带单测）
- [x] release.yml：mac --bundles app,dmg 产出 .app.tar.gz(+.sig)、win NSIS(+.sig)，
      每平台 latest-<platform>.json → release job 合并 latest.json 一起发布
- [x] 公私钥都不入库：tauri.conf.json pubkey 留空占位，CI 构建时用 --config 注入
- [x] __APP_VERSION__ 由 vite define 注入（package.json version 为准）

### v4（Microsoft OAuth2 设备码登录）
背景：用户实测 Exchange Online（wanchain.org）密码登录报 `AUTHENTICATE failed`——
微软官方文档明确 Outlook.com / Exchange Online 的 IMAP/POP/SMTP "requires the use of
Modern Auth / OAuth2"，基本认证已停用，应用密码也不行。
- [x] `oauth.rs`：RFC 8628 设备码流程（begin/poll）+ refresh_token 自动刷新 + XOAUTH2 SASL 串；
      默认 client_id 用 Thunderbird 公共客户端（已实测 /common 支持设备码），UI 可改填自有 Azure 应用
- [x] 三协议接入：IMAP `AUTHENTICATE XOAUTH2`（二次挑战回空串拿最终错误）、
      POP3 `AUTH XOAUTH2 <base64>`、SMTP lettre `Mechanism::Xoauth2`
- [x] `Account.auth`（password|oauth2，serde 默认 password 兼容旧数据）；
      `AccountSecret.oauth: Option<OAuthTokens>` 存 secrets.json（0600）
- [x] `fresh_secret()`：所有连接前检查 access_token（到期前 2 分钟）自动刷新并回写
- [x] AccountModal：Exchange 预设默认 OAuth2 模式——「用 Microsoft 账户授权」→ 自动开浏览器
      （microsoft.com/devicelogin）→ 大字号显示设备代码 → 轮询直到授权成功 → 测试/保存
- [x] 测试：oauth 单测 2 个（SASL 串格式、令牌解析/刷新沿用旧 refresh_token/缺失报错）

### v5（实测反馈修复：回复全部 / 目录名乱码 / 验证面板折叠）
用户用 Exchange 真实账户实测（OAuth 登录成功）后反馈三个问题：
- [x] 回复全部：EmailFull 增加 cc（mail.rs 解析 Cc 头）；MessageView 新增「回复全部」按钮；
      To = 原发件人 + 原 To（去自己、去重），Cc = 原 Cc（去自己）
- [x] 目录名乱码：IMAP 目录名是 modified UTF-7（RFC 3501 §5.1.3，如 &T797Og-）；
      imap_client.rs 实现 decode/encode_mutf7（3 个单测含 RFC 官方示例「台北」=&U,BTFw-）；
      display 解码显示，与服务器交互仍用原始名；create_folder 创建中文目录时编码
- [x] 验证面板太显眼：默认折叠成 54px 窄条（小封印 + 竖排状态字），点击展开完整面板，
      展开后右上角 » 收起；偏好存 localStorage("sealmail.railOpen")

### v6（自己签名显示绿色 + macOS 关闭即隐藏）
- [x] 自己签的邮件直接绿色「已验证」：store.rs::trusted_for_verify() 在校验用可信列表里
      附加本机身份（名字「{显示名}（本人）」+ active_fingerprint），fetch_messages 使用；
      其他人冒用该密钥地址/换密钥仍会触发 impersonation；新增 e2e 测试（先验证不注入时是黄色）
- [x] 关闭按钮行为（参考 auto-desktop）：AppPrefs{close_behavior} 存 prefs.json，
      macOS 默认 "hide"（其他平台 "quit"）；on_window_event 拦 CloseRequested → prevent_close + hide；
      RunEvent::Reopen（cfg macos）点程序坞图标恢复窗口；Cmd+Q 正常退出；
      get/set_close_behavior 命令 + KeysView「关于与更新」卡片里的下拉设置

### v7（新邮件自动推送）
- [x] watcher.rs：每账户一个后台线程——IMAP 常驻连接 + RFC 2177 IDLE
      （EXAMINE INBOX → idle().wait_with_timeout(4min) × 6 轮/连接，之后重连顺带刷新 OAuth 令牌）；
      POP3 无推送 → 每 2 分钟 STAT 轮询；exists 增加才 emit "new-mail"（删除邮件不触发）
- [x] oauth.rs 增加 refresh_tokens_blocking（监听线程无 async 运行时；reqwest 加 blocking feature）
- [x] 线程生命周期：RUNNING 集合去重；账户删除后线程下一轮自检退出；
      出错 30s 退避重连；setup 与 add_account 后调用 ensure_watchers
- [x] 前端 App.tsx listen("new-mail")：当前账户匹配则自动 loadMessages（未读数/列表即时更新）

### v8（阅读窗一键信任发件人）
- [x] 信任模型说明（用户问"怎么知道密钥就是对方本人"）：签名只证明密钥一致性 + 内容完整，
      密钥↔人 的绑定靠 TOFU（首次信任）+ 持续性监测（换密钥即标红 impersonation），
      与 SSH known_hosts / Signal 安全码同模型；真正核实只能走带外渠道（电话/微信对指纹）
- [x] MessageView：signedUnknown 时发件人地址下方出现金色「✓ 信任此发件人」chip
      （不再必须展开验证面板）→ 点击展开轻量确认卡：地址 + 指纹（可复制）+
      带外核实建议 + 「确认信任」/「取消」；确认走原 trust_sender 流程，封印即刻变绿
- [x] 换邮件自动收起确认卡（useEffect on uid）；styles.css 新增 .trust-chip / .trust-confirm

**GitHub Secrets（用户手动配置，密钥文件在本机 ~/.tauri/）**：
- `TAURI_UPDATER_PUBKEY` = ~/.tauri/sealmail-updater.key.pub 的内容（公钥，构建时注入 tauri.conf）
- `TAURI_SIGNING_PRIVATE_KEY` = ~/.tauri/sealmail-updater.key 的内容（私钥，签 updater 工件）
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` = 空字符串（生成时未设密码）
注意：发版时 package.json + tauri.conf.json + Cargo.toml 三处 version 要一起改，tag 用 v<version>。

## 待办 / 路线图（2026-06-11 产品 review 后重排，定位「小而美」）

### P0 — 没有就当不了主力客户端

1. **SQLite 本地缓存 + 增量同步**（病根：目前每次全量拉最近 30 封 BODY.PEEK[]，
   无本地存储 → 切目录等网络、只有 30 封、断网全瞎、搜索范围小。
   方案：rusqlite mail.db；IMAP 按 UIDVALIDITY+UID 增量、近窗口同步 FLAGS、检测删除；
   POP3 用 UIDL；列表秒出本地数据+后台刷新+加载更多）
2. **删除安全**：删除 = 移入"已删除"目录（IMAP 找/建 Trash），永久删除仅限 Trash 内且弹确认
   （现状是 \Deleted+EXPUNGE 直接物理删除，手抖即丢信）
3. **HTML 正文安全渲染**：sanitize 后入 sandbox iframe，默认阻止远程图片（防追踪），
   一键"加载图片"；链接跳系统浏览器（现状只渲染纯文本，现代邮件大半没法看）
4. **附件**：下载保存收到的附件；写信可添加附件
5. **联系人自动收集 + 收件人自动补全**：从收发历史静默建表（本地），To/Cc 输入下拉补全。
   不做完整通讯录页面——"输两个字母就出来"才是刚需本体
6. **草稿自动保存**：写一半关掉不丢；侧栏草稿入口可恢复

### P1 — 好用与否的分水岭

7. **撤销发送**：点发送后本地倒计时 10s 再真正走 SMTP，期间可一键撤销
8. **键盘快捷键**：Cmd+/-/0 字号缩放（存偏好）、Cmd+N 写信、Cmd+R 回复、
   ↑↓/j/k 切邮件、Delete 删除、Cmd+F 聚焦搜索
9. **未读过滤 + 标为未读 / 全部已读**（列表头"全部/未读"切换）
10. **MessageView 显示 To/Cc 收件人列表**（数据已有，未渲染；回复全部前应能看到都有谁）
11. **新邮件系统通知**（tauri-plugin-notification，窗口隐藏/失焦时弹横幅，可在设置关闭）
12. **星标/旗标**（IMAP \Flagged；POP3 本地记录）

### P2 — 缓存落地后再做

13. 会话线程视图（References/In-Reply-To 聚合）
14. IMAP 服务器端搜索（本地缓存已覆盖大部分场景后补盲区）
15. 自定义签名档文本、归档一键操作、统一收件箱
16. Gmail OAuth2（需注册 Google Cloud 客户端；Gmail 应用专用密码目前仍可用）
17. 多语言 UI；macOS 公证（APPLE_* secrets 同 auto-desktop）

### 明确不做（守住小而美）

富文本编辑器（坚持纯文本写信 + HTML 阅读）、日历、待办、RSS、AI 摘要、
已读回执、邮件模板、定时发送。

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
