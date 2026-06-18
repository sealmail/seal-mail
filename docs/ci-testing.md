# SealMail CI Testing

## 必跑 CI

`CI / Core + CLI + frontend types` 会在 pull request 和 push 到 `main` 时自动执行：

- `cargo check`
- `cargo test --tests`
- `bunx tsc --noEmit`

这层不依赖 Docker、不依赖真实邮箱、不依赖 secrets。它覆盖 Core 本地状态、CLI 契约、签名/验签、MIME 解析、过滤器、风险识别等稳定能力。

## 手动 Docker 邮件服务 smoke

`CI / Docker mail server smoke` 是手动触发 job，用 GreenMail Docker 服务检查本地 SMTP/IMAP/POP3 测试服务能在 GitHub Actions runner 内启动并暴露端口。

它现在只做服务可用性 smoke，不冒充完整协议 E2E。原因是当前生产 IMAP/POP3 客户端只走 TLS，本地测试服务默认自签证书；在没有测试证书策略前，不应该为了 CI 放宽生产 TLS 行为。

后续把它升级为 required E2E 前，需要先完成：

- 测试邮件服务 TLS 证书方案，或测试专用受信根证书导入方案
- SMTP → IMAP 读回链路
- IMAP 目录、移动、删除、已读、星标链路
- POP3 拉取和本地虚拟目录链路
- 附件、中文编码、HTML 邮件和认证失败用例

## 如何触发

自动触发：

- 打开或更新 pull request
- push 到 `main`

手动触发 Docker smoke：

1. 打开 GitHub 仓库的 **Actions**。
2. 选择 **CI** workflow。
3. 点击 **Run workflow**。
4. 勾选 `Run the optional Docker-backed mail server smoke job`。
5. 点击绿色的 **Run workflow**。

命令行触发：

```bash
gh workflow run CI --ref main -f run_mail_server_smoke=true
```

只跑普通必跑 CI 时，不需要传入 `run_mail_server_smoke`。
