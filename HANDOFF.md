# HANDOFF — SealMail 信印

> 工作交接/进度文档。**每次修改代码后必须同步更新本文件。**
> 最后更新：2026-07-08（v24：过滤规则「发件人等于地址」修复）

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

### v9（产品 review 后 P0/P1 一次性落地，9 个 commit）
- [x] **SQLite 本地缓存 + 增量同步**（db.rs，rusqlite bundled，mail.db）：
      存原始 RFC822 + 少量列（unread/flagged/timestamp/pop_uidl）；**读取时重新解析+验证**，
      信任列表变化无需迁移缓存即可生效。IMAP：UIDVALIDITY 变化全量重建，否则先 UID 探测再拉新邮件全文，
      FLAGS 回扫最近 200 封（已读/星标/服务器侧删除检测）；POP3：UIDL 识别、本地自增 uid、
      目录/已读/星标全在本地列（弃用 local_assign/local_read 旧路径）。
      前端 list_cached 秒出（离线可用）+ sync_messages 后台同步 + 「加载更早的邮件」分页；
      同步失败显示细条提示，缓存列表不消失
- [x] **删除安全**：删除=移入回收站（LIST 找 \Trash 特殊属性或常见名，找不到就建 "Trash"；
      POP3 用本地「已删除」虚拟目录）；回收站内删除才物理删且弹确认框
- [x] **HTML 正文渲染**（HtmlBody.tsx）：DOMParser 消毒（去 script/iframe/form/on*/javascript: 等）
      → sandbox iframe（仅 allow-same-origin，无脚本）；远程图片默认阻止（防追踪像素）可一键加载；
      链接经 opener 跳系统浏览器。**已签名邮件默认仍显示纯文本（签名 canon 只覆盖文本，所见即所验）**，
      可手动切 HTML 并带警示；未签名邮件默认 HTML
- [x] **附件**：阅读窗每个附件可「保存」（优先用本地缓存原文，缺失回源；POP3 用 UIDL 定位）；
      写信可添加多个附件（tauri-plugin-dialog 选文件，后端读取，mail-builder 按扩展名猜 MIME）。
      注意：附件不在签名哈希范围内
- [x] **联系人自动收集 + 补全**（contacts.json）：收信记发件人、发信记收件人（次数+最近往来）；
      写信 To/Cc 输入片段即出下拉（AddrInput.tsx，键盘 ↑↓/Enter/Tab）
- [x] **草稿**（drafts.json）：写信防抖 800ms 自动保存，关窗前再 flush；发送成功自动删除；
      侧栏「草稿」入口（DraftsPane）可恢复/删除
- [x] **撤销发送**：点发送先进 10 秒倒计时（标题+底部提示），期间可「↩ 撤销」或「立即发送」；
      倒计时中点 × 只取消发送不关窗
- [x] **快捷键**：Cmd+/-/0 缩放（body zoom，localStorage 持久）、Cmd+N 写信、Cmd+R 回复、
      Cmd+Shift+R 回复全部、Cmd+F 聚焦搜索、↑↓/j/k 切邮件、Delete 删除（输入框聚焦时不抢键）
- [x] **未读/星标**：列表头「全部/未读/星标」三段切换；全部已读（mark_read 批量单连接）；
      标为未读按钮；星标 IMAP \Flagged 双向同步（回扫窗口内），POP3 本地记录
- [x] **To/Cc 显示**：阅读窗头部显示收件人/抄送列表
- [x] **新邮件系统通知**（tauri-plugin-notification）：watcher 检测到新邮件且窗口未聚焦时弹横幅，
      设置页可关（prefs.notify_new_mail）
- [x] **点击通知定位邮件（macOS 根治：自己捕获点击）**：
      根因——tauri-plugin-notification 在 macOS 用废弃的 `NSUserNotification` 且 `wait_for_click=false`，
      发完即走、不监听点击；`actionPerformed` 又只在移动端触发。所以点通知后系统既不激活 App
      也不回调，A/B 场景全都没反应（与 dev 无关，正式版同样）。已用 `examples/notify_probe.rs`
      实测确认 `mac_notification_sys` + `wait_for_click(true)` 能拿到 `Click`。
      现 macOS 改为自己发通知：watcher `spawn_macos_click_notification` 在后台线程
      `set_application(bundle)` + `wait_for_click(true)` 阻塞 `send()`，捕获 `Click` 后**自己**
      `reveal_main_window` + 设定目标 + emit `notification-activated`，完全不依赖系统激活/RunEvent。
      只有真点击才动作（普通聚焦不误触发）。其它平台仍走 tauri-plugin-notification + 前端拉取。
      新增直接依赖 `mac-notification-sys`（仅 macOS target）。前端保持「只注册一次 + ref 取最新
      openNotificationMail + 多信号拉取（mount/focus/visibilitychange/notification-activated）」。
      不变量由 `watcher::tests::pending_notification_target_lifecycle` 锁定。
- [x] **性能：列表/启动/切换卡顿（正式版白屏十几秒、切邮件卡）**：
      根因——DB 只存原始 raw（含附件，可能几百 MB），列表(`list_cached`)、会话(`list_thread`)每次都
      把整批 raw 读出来逐封 MIME 解析+验证。修复：
      ① DB 新增 `meta_json` 列（安全幂等迁移：`ALTER TABLE ADD COLUMN`，旧行惰性回填）。
      `list_cached`/`list_thread` 改走轻量 `list_meta`/`list_folder_meta`（只读元信息 JSON、不读 raw），
      命中即秒出；未命中才读一次 raw 解析并写回。已读/星标以 DB 列为准；`upsert`/`set_folder` 会
      失效该行缓存；可信联系人变更时 `invalidate_meta_cache` 清空全部（trust 按新规则重算）。
      ② 前端 `selectMail` 优先用已加载列表本地筛同会话，仅本地缺失时才回退后端。
      注意：更新后**首次**打开仍会回填一次（解析首屏），之后秒开。
      新测试 `db::tests::meta_json_cache_lifecycle`（写回/重置/清空/移动目录失效）。

### v10（P2：会话线程视图）
- [x] `mail.rs` 解析 `Message-ID` / `References` / `In-Reply-To`，生成 `message_id` 与 `thread_id`；
      线程根优先取 References 首个 ID，其次 In-Reply-To，其次自身 Message-ID，最后用规范化主题兜底
- [x] `list_thread` Tauri 命令基于本地 SQLite 缓存扫描当前目录同线程邮件，按时间正序返回
- [x] `MessageView` 在正文上方显示会话时间线，可点击同线程其他邮件跳转；当前邮件高亮，未读发件人加粗
- [x] 测试：`parses_conversation_headers` 覆盖 References/In-Reply-To 聚合规则

### v11（P2：归档一键操作）
- [x] IMAP：识别 `\Archive` special-use、Archive/All Mail/归档等常见目录；找不到则创建 `Archive`
- [x] POP3：归档到本地「归档」虚拟目录，并自动加入侧栏目录列表
- [x] 后端新增 `archive_message` 命令；前端阅读窗新增「归档」按钮，归档后清空选中邮件、刷新列表和目录
- [x] 已在归档目录内隐藏「归档」按钮；IMAP 同目录归档后端 no-op，避免误删本地缓存行

### v12（P2：统一收件箱）
- [x] 侧栏新增「统一收件箱」虚拟目录，聚合所有账户本地缓存里的 INBOX 邮件并按时间倒序展示
- [x] 打开统一收件箱时并行同步所有账户 INBOX；新邮件推送来自任一账户时自动刷新
- [x] 列表行显示账户邮箱标签；选中、已读、星标、键盘上下移动改用 account/folder/uid 三元键，避免跨账户 UID 撞号
- [x] 统一收件箱支持批量全部已读、加载更多；隐藏「移动到…」以避免跨账户误移动，保留归档/删除/星标/回复等单封操作

### v13（UI 配色中性化）
- [x] `styles.css` 主设计变量从暖黄纸感切到 Codex 风格的中性灰白底、近黑文字、低饱和绿色强调
- [x] 侧栏、标题栏、列表、阅读窗、线程条、弹层、设置页、欢迎页等大面积背景/边框/hover/active 色统一去黄
- [x] 组件内联颜色同步改为 CSS 变量（写信弹窗、账户/OAuth、Ledger、身份密钥、档案、风险弹窗、验证栏等）
- [x] 保留风险红、签名金、可信绿的语义色，但降低大面积黄色占比

### v14（缩放滚动修复 / v0.1.8）
- [x] Cmd/Ctrl +/-/0 缩放后广播 `sealmail-zoom-change`，让 HTML 邮件 iframe 重新测量高度
- [x] HTML 邮件 iframe 按正文真实高度展开，不再用未缩放的 `window.innerHeight` 截断内容
- [x] iframe 监听窗口 resize、字体加载、图片加载和 ResizeObserver，避免放大字体后底部内容滚不到

### v15（独立邮件窗口阅读列居中 / v0.1.9）
- [x] 独立邮件窗口内的工具栏、HTML iframe、纯文本正文统一使用居中的阅读列宽
- [x] 覆盖纯文本正文原本的 `max-width: 640px` 靠左布局，减少宽窗口右侧空白
- [x] HTML 模式 iframe 不再铺满整窗宽度，避免邮件背景在右侧形成大块白边
- [x] 默认展示策略改为：只要邮件包含 HTML 正文，就默认使用 HTML 模式，用户仍可切换纯文本

### v16（邮件列表自动分类 / v0.1.10）
- [x] 新增本地自动分类规则：个人 / 商务 / 广告，不移动远端邮件目录
- [x] 邮件列表新增分类切换栏与分类计数，可按三类筛选展示
- [x] 邮件行新增分类标签，搜索、未读、星标过滤可与分类筛选叠加

### v17（界面缩放窗口补偿 / v0.1.11）
- [x] 缩放快捷键同步写入 `--sealmail-zoom`，供 CSS 计算补偿尺寸
- [x] body 按 `1 / zoom` 扩展布局宽高，避免缩小后右下露白
- [x] 主窗口和独立邮件窗口改为占满补偿后的根容器，避免放大后底部设置等控件被裁掉

### v18（子窗口快捷键与会话折叠 / 未发布）
- [x] 独立邮件窗口增加捕获层快捷键处理，确保 Cmd/Ctrl +/-/0 可调整字体缩放
- [x] HTML iframe 内触发缩放快捷键时同步通知父窗口，避免焦点在邮件正文里时失效
- [x] 超过 3 封的会话默认折叠中间邮件，只保留首封和末尾两封；点击可展开全部

### v19（通知点击根治 + UI 响应优化 + 持久化日志 / v0.1.34）

**macOS 通知点击（第三次修复，这次有 E2E 证据）**：
- [x] 根因：v0.1.33 的 mac-notification-sys `wait_for_click` 方案有先天缺陷——每次
      `send()` 都会**替换** NSUserNotificationCenter 的全局 delegate，且回调不校验是
      哪条通知。邮件客户端常态是多条通知并存，点任何一条都只会唤醒最后一条的等待线程：
      要么打开错误的邮件，要么毫无反应。另外用户当时装的是 v0.1.32（修复只进了 v0.1.33），
      测到的还是「完全没反应」的旧版行为。
- [x] 新 `src/mac_notify.rs`：objc2 自定义**单例 delegate**（进程内只装一次）+ 每条
      通知唯一 identifier + identifier→NotificationMailTarget 注册表；主线程回调按
      id 精确路由，不阻塞任何线程；点「通知中心里攒下的旧通知」也各开各的邮件
      （限本进程发出的；重启后点旧通知只唤起窗口）。mac-notification-sys 仅保留
      `set_application`（dev 未打包运行时伪装 bundle 身份；打包后跳过 swizzle）。
      前端拉取模型（open_pending_notification_mail）不变。
- [x] 通知打开改**本地缓存优先**：openNotificationMail 先 listCached 定位直接打开
      （实测点击→邮件打开约 200ms），找不到才阻塞同步一次；目录刷新/列表同步放后台。
      旧实现先 refreshFolders+syncMessages（网络）再打开，Exchange 慢时点了通知
      几秒都打不开（实测挂过 8 分钟）。
- [x] 测试：`mac_notify::tests::identifier_routing_is_exact_and_consume_once` 锁定
      「按 id 精确路由/取走即消费/未知 id 只唤窗口」不变量（ObjC 回调层无法无头单测，
      注册表就是路由正确性的测试缝隙）。

**UI 响应优化（用户反馈"有点慢"，实测定位到三个来源）**：
- [x] **cli_json 同步命令冻结主线程（最大头）**：Tauri 2 同步命令在主线程执行，GUI
      所有业务调用（走 CLI 子进程桥）都会阻塞 UI 事件循环——同步一次邮件整窗卡死
      2 秒+。改 async + `spawn_blocking`：主线程始终响应，独立调用并行
      （实测启动 state/drafts 并行，点通知后 read 不再排在 sync/folders 后面）。
- [x] **会话正文懒加载**：selectMail 原来 Promise.all 一次性拉整条会话所有正文
      （长会话数十次 read+MIME 解析+验签）。现在只急加载选中封+首封+末尾三封
      （≤6 封才全量），其余渲染占位卡片（发件人/日期/预览）点击再取；
      同会话内切换复用 metas 与正文缓存，不再整屏闪烁重取。
- [x] **meta 缓存后台补全**：升级后首次访问的懒回填墙（实测 2700 封目录首次
      list_thread 卡 4.2 秒）挪到启动 5 秒后的后台线程小批推进
      （`core::backfill_meta_batch`，批 40 封间隔 25ms，让出锁与 IO）；
      已删除账户的残留行自动跳过。SQLite 加 `busy_timeout(5s)`，
      GUI 进程与 CLI 子进程并发读写不再怕撞锁。

**持久化日志（真机排障用）**：
- [x] `src/logging.rs`：写 `<应用配置目录>/logs/sealmail.log`，单文件 1MB 超限轮转为
      `.log.old`。记录通知链路 `[notify]`、GUI 全部 CLI 调用耗时 `[perf]`、
      缓存补全 `[cache]`。只在用户动作级别写，不进热循环，无性能影响。
      macOS 查看：`tail -f ~/Library/Application\ Support/com.sealmail.app/logs/sealmail.log`
- [x] 调试钩子：`SEALMAIL_DEBUG_NOTIFY=<account_id>:<uid>[,<uid>...]` 启动应用，
      8 秒后走真实链路发指向已缓存邮件的测试通知（该模式下绕过窗口聚焦检查），
      真机不用等真实新邮件即可验证「点通知→跳转邮件」。account_id 与 uid 可从
      日志或 `sealmail-cli list` 获取。

### v20（界面缩放三连修 + 原生缩放菜单 + 拦截提示弱化 / v0.1.35）

**界面缩放布局全面失效（右侧留白 / 横向滚动 / 内部滚动条闪烁）**：
- [x] 根因（Safari 探针实测，非推断）：新版 WebKit 实现了标准化 CSS zoom——
      **百分比宽度自动按 zoom 换算**（100% 永远贴合父级），但 **vw/vh 不换算**
      （zoom 1.4 时 100vw 渲染成 1.4 倍窗宽直接溢出）。旧代码按旧引擎行为写的
      `width: calc(100% / zoom)` / `calc(100vw / zoom)` 补偿全部反向破坏：
      放大→正文缩成 1/zoom（右侧留白），缩小→撑成 1/zoom 倍宽（横向滚动），
      高度公式随之失配（内部纵向滚动条反复出现→闪烁）。
- [x] 修复：删掉全部 zoom 宽度补偿（外层 body 与 iframe 内 body 都用 100%）；
      `.search-wrap` 的 100vw 上限改 100%。
- [x] iframe 高度公式改为**坐标系抵消**：只用 iframe 内 body 自身坐标系（不含
      zoom）的 scrollHeight/offsetHeight 直接做样式高度——iframe 元素高度被外层
      zoom 放大一次、内容被内层 zoom 放大一次，因子相同正好抵消，任意缩放精确贴合，
      无需除法。**切记 documentElement.scrollHeight 是含 zoom 的渲染像素，不能混用**。
      另给 iframe 根元素加 `overflow-y: hidden`（高度由父页面精确设置，内部
      永远不该出现纵向滚动条），横向仍允许滚动兜底超宽邮件。
- [x] 探针方法：`zoom` 语义在文档里查不到实现细节，写独立 HTML 探针在 Safari
      （同系统 WebKit）里实测 gBCR/scrollHeight/clientWidth 再定公式；
      以后遇到引擎行为分歧照此办理，别靠猜。

**缩放快捷键在 app 切换后失效**：
- [x] 根因：WKWebView 在应用失焦再激活后可能不恢复 first responder，页面收不到
      keydown。修复：macOS 在默认菜单 View 里插入 实际大小/放大/缩小
      （Cmd+0/=/-）原生菜单项（lib.rs `setup_zoom_menu`），on_menu_event 发
      `sealmail-menu-zoom` 事件给**当前聚焦窗口**，前端 useZoomShortcuts 统一应用。
      原生加速键不依赖 webview 焦点，弹窗/主窗都有效；keydown 路径保留给 Windows。

**远程内容拦截提示弱化**：
- [x] 黄色整行横幅（占正文上方视线焦点）改为悬浮在正文右上角的半透明小胶囊
      `.img-blocked-chip`（「已阻止远程图片 · 显示」，hover 提高不透明度），
      追踪原理说明移进 title 提示，不再占版面。

**教训（勿重蹈）**：
- `cargo build` 出来的 debug 二进制加载的是**编译时嵌入的 dist/**（不连 vite），
  真机验证前端改动必须用 `bunx tauri dev`（或先 `bun run build` 刷新 dist 再重编）；
  否则测的是旧代码还浑然不觉——本轮就踩了，靠界面上的旧文案才发现。

**GitHub Secrets（用户手动配置，密钥文件在本机 ~/.tauri/）**：
- `TAURI_UPDATER_PUBKEY` = ~/.tauri/sealmail-updater.key.pub 的内容（公钥，构建时注入 tauri.conf）
- `TAURI_SIGNING_PRIVATE_KEY` = ~/.tauri/sealmail-updater.key 的内容（私钥，签 updater 工件）
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` = 空字符串（生成时未设密码）
注意：发版时 package.json + tauri.conf.json + Cargo.toml 三处 version 要一起改，tag 用 v<version>。

### v21（链接一律走系统浏览器 + WKWebView confirm 失效根治 / v0.1.37）

**点邮件内链接在邮箱内跳转（应开系统浏览器）**：
- [x] 根因一：**wry 的 WKWebView 里 `window.confirm` 是 no-op（不弹窗、静默返回
      false）**，url.ts 里"是否用系统浏览器打开"的确认从未真正工作过，整条
      openExternalUrl 链路被它挡死。改用 `@tauri-apps/plugin-dialog` 的 `ask()`；
      **以后一切浏览器原生对话框（alert/confirm/prompt）都必须走 dialog 插件**。
- [x] 策略调整：正常链接直接开系统浏览器不弹窗；仅链接文字域名与实际指向不符
      （钓鱼手法）时弹 ask 警告。
- [x] 根因二兜底：srcDoc iframe 的 load 可能先于 React 绑定完成（竞态），点击监听
      挂不上时链接走默认导航（iframe 内原地跳转）。三层防御：
      ① 消毒时给所有 `<a>`/`<area>` 强制 `target="_blank" rel="noreferrer noopener"`
      ——沙箱未开 allow-popups，默认动作被引擎拦成无操作，物理杜绝原地跳转；
      ② 挂载后若文档已 complete 主动补一次 onLoad 绑定；
      ③ 点击拦截扩展到 `<area>` 图片热区。
- [x] 同根因连带修复：MessageView 危险附件警告的 `window.confirm` 也失效
      （危险附件永远无法下载），一并换 dialog `ask()`。

### v22（打开会话整条标已读 / v0.1.38）

**多封未读的会话反复点开也清不掉未读点**：
- [x] 根因：列表行按会话（threadId）聚合，未读点是"会话内任意一封未读"就亮；
      但 `selectMail` 只把被点击的最新一封标已读，会话里更早的未读邮件永远
      不会被标记。且再次点击时最新一封已是已读（`m.unread` 为 false），
      连标记逻辑都不再触发——看多少遍都无效。
- [x] 修复（App.tsx `selectMail`）：会话 metas 加载后，把整条会话的未读邮件
      通过既有 `api.markRead` 批量接口一次标已读（与 Gmail 会话行为一致），
      并同步 messages / inboxMetas / thread 本地状态。
      通知直达路径（`markRead: false`）不受影响。

### v23（邮件子窗口缩放局部化 / v0.1.39）

- [x] 邮件弹出子窗口（popout）的快捷键/菜单缩放改为**局部、临时**：
      `useZoomShortcuts` 增加 `persist` 开关，PopoutApp 传 `persist: false`——
      初始值仍继承主窗口的 `sealmail.zoom`，但子窗口里的调整不再写 localStorage，
      关窗即弃，不会污染主窗口下次启动和后续新开子窗口的缩放。
      （原生 setZoom 本就按窗口生效，菜单缩放本就只发给焦点窗口，泄漏点仅落盘这一处）

### v24（过滤规则「发件人等于地址」修复 / v0.1.40）

- [x] `filters.rs` rule_matches：from 字段原先把显示名和地址拼成 `"名 地址"` 一个串，
      「等于 某地址」全串比较永远不命中（用户的 jenkins 规则一封都匹配不到）。
      改为 from 拆成显示名/地址两个候选串、to 拆成各收件地址，任一候选命中即匹配；
      `not_contains` 要求所有候选都不包含。equals 仍不做子串匹配。
      TDD：`tests/core.rs` 新增 `filter_from_equals_matches_address` 先红后绿。

## 待办 / 路线图（2026-06-11 产品 review 后重排，定位「小而美」）

P0/P1 全部 12 项已在 v9 落地，P2 第 13 项和第 15 项中的「归档」「统一收件箱」已在 v10-v12 落地（见上）。剩余：

### P2 — 下一批

14. IMAP 服务器端搜索（本地缓存已覆盖大部分场景后补盲区）
15. 自定义签名档文本
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
