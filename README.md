# SealMail — A Trusted Mail Client

English | [中文](README.zh-CN.md)

A general-purpose desktop email client (Tauri 2 + Vite + React + TypeScript) whose signature feature is **email signing and trust verification**: a normal mail client handles "sending and receiving"; SealMail additionally **proves an email can be trusted**.

The UI is implemented from a Claude Design mockup (`SealMail.dc.html`): a wax-seal trust metaphor, three-pane layout with a verification rail, high-risk interception, a signing compose flow, identity & key management, and sender trust profiles.

## Features

### General mail client (the foundation comes first)
- **Multiple accounts**: IMAP / POP3 for incoming + SMTP for outgoing (SSL / STARTTLS)
- **Provider presets**: Exchange Online (Office 365), on-prem Exchange Server, Gmail, iCloud, QQ Mail, NetEase 163, plus custom IMAP/POP3
- Inbox / server folder browsing, read/unread, reply / forward / move / delete, local search
- **Custom folders**: real server-side folders for IMAP accounts; local virtual folders for POP3
- **Filter rules**: match on from/to/subject/body × contains/equals/starts-with/ends-with → auto-move into a folder (optionally mark read), with one-click "organize inbox now"

### Trust layer (the signature feature)
- A locally generated Ed25519 identity key (`identity.key`, mode 0600; the private key never leaves your machine)
- Optional signing on send: signature data lives in `X-SealMail-*` mail headers and is **invisible to ordinary recipients**; the body only gains one low-key line in the standard `-- ` signature-block format, so it never disrupts reading in a regular mailbox
- Local verification on receive, with five states (wax-seal semantics):
  - 🟢 **Intact seal** — verified sender (valid signature + fingerprint matches your trusted record)
  - 🟡 **Gold seal** — valid signature, not yet trusted (one click to add to trusted contacts)
  - ⚪ **Empty ring** — unsigned, identity unknown
  - 🔴 **Cracked seal** — content tampered (body hash doesn't match the signature)
  - 🔴 **Forged seal** — impersonating a known contact (same display name, wrong key/domain)
- High-risk warnings: heuristic detection for payments (funds + urgency wording), account security (requests for seed phrases / passwords), and contracts (clauses + deadlines); the risk modal requires checking "independently verified" before proceeding
- When no account is configured, the six demo emails from the design mockup are shown (covering every trust state)

## Getting started

```bash
bun install
bun run tauri dev      # development
bun run tauri build    # package
```

Tests:

```bash
cd src-tauri && cargo test   # end-to-end tests: sign/verify, tamper, impersonation, filters, risk detection
bunx tsc --noEmit            # frontend type check
```

## Exchange notes

- **Exchange Online / Office 365**: preset uses `outlook.office365.com:993` (IMAP) + `smtp.office365.com:587` (STARTTLS).
  Microsoft has retired basic auth — an admin must enable IMAP + SMTP AUTH; personal accounts should use an app password.
  An OAuth2 (XOAUTH2) device-code flow is on the roadmap.
- **On-prem Exchange Server**: once an admin enables the IMAP/POP3 services, just enter your company server address. Native EWS/Graph protocols are on the roadmap.

## Architecture

```
src-tauri/src/
  lib.rs          Tauri command layer (accounts/folders/messages/send/filters/trusted contacts)
  models.rs       Data models (Account, EmailMeta/Full, VerifyDetail, FilterRule…)
  crypto.rs       Ed25519 sign/verify, fingerprints, body canonicalization, X-SealMail-* headers
  mail.rs         MIME parsing (mail-parser), trust evaluation, risk/language detection
  imap_client.rs  IMAP (connect/folders/fetch/move/read/delete; falls back to COPY+DELETE when MOVE is unsupported)
  pop3_client.rs  Minimal POP3 over TLS (local virtual-folder filing)
  smtp_client.rs  SMTP sending (lettre) + MIME building (mail-builder) + signed headers
  filters.rs      Filter-rule matching engine
  store.rs        Local persistence (accounts / secrets(0600) / filters / trusted / local folders)
src/
  App.tsx         Main shell (three panes + verification rail + overlays)
  trust.ts        Trust-state copy / check rows / risk-banner mapping
  demo.ts         The six demo emails from the design mockup
  components/     Seal (wax-seal renderer), Sidebar, MailList, MessageView, VerifyRail,
                  ComposeModal, AccountModal, FiltersModal, KeysView,
                  ProfileSlideOver, RiskModal
```

## Security notes

- Account passwords are stored in `secrets.json` in the local app-config directory (mode 600), never inside the project tree, and never committed to git
- The signing private key `identity.key` is handled the same way; all signing/verification happens locally
- Verification never relies on avatars, header decorations, or language — only key fingerprints

## Roadmap

- IMAP IDLE push, local mail cache (SQLite)
- OAuth2 / XOAUTH2 (Exchange Online, Gmail)
- Hardware-key signing (Ledger / YubiKey)
- Attachment download/upload, safe HTML body rendering
- Native EWS / Microsoft Graph for Exchange
