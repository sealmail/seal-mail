import { invoke } from "@tauri-apps/api/core";
import type {
  Account,
  AccountSecret,
  AppStateView,
  ApplyResult,
  BrowserFlowStart,
  Contact,
  Draft,
  DeviceFlowStart,
  DevicePoll,
  EmailFull,
  EmailMeta,
  FilterRule,
  FolderInfo,
  IdentityInfo,
  LedgerAccountRow,
  OAuthProvider,
  OAuthTokens,
  SendResult,
  TrustedContact,
} from "./types";

type CliEnv = Record<string, string>;

interface AppPrefsJson {
  closeBehavior: "hide" | "quit";
  notifyNewMail: boolean;
}

type CliArg = string | number | boolean;

function pushFlag(args: CliArg[], name: string, value: CliArg | null | undefined) {
  if (value === null || value === undefined) return;
  args.push(name, String(value));
}

function cliJson<T>(args: CliArg[], stdin?: unknown, env?: CliEnv): Promise<T> {
  return invoke<T>("cli_json", {
    args: args.map(String),
    stdin: stdin === undefined ? null : JSON.stringify(stdin),
    env: env ?? null,
  });
}

export async function getState(): Promise<AppStateView> {
  return cliJson<AppStateView>(["state"]);
}

// ── 身份 / Ledger ──
export async function ledgerGetAddresses(count = 5): Promise<LedgerAccountRow[]> {
  return invoke("ledger_get_addresses", { count });
}

export async function bindLedger(path: string, address: string): Promise<IdentityInfo> {
  return cliJson(["identity", "bind-ledger", "--ledger-path", path, "--address", address]);
}

export async function useLocalKey(): Promise<IdentityInfo> {
  return cliJson(["identity", "use-local"]);
}

// ── 偏好 ──
export async function getCloseBehavior(): Promise<"hide" | "quit"> {
  const prefs = await cliJson<AppPrefsJson>(["prefs"]);
  return prefs.closeBehavior;
}

export async function setCloseBehavior(behavior: "hide" | "quit"): Promise<"hide" | "quit"> {
  const result = await cliJson<Pick<AppPrefsJson, "closeBehavior">>(["pref", "set", "--close-behavior", behavior]);
  return result.closeBehavior;
}

export async function getNotifyNewMail(): Promise<boolean> {
  const prefs = await cliJson<AppPrefsJson>(["prefs"]);
  return prefs.notifyNewMail;
}

export async function setNotifyNewMail(enabled: boolean): Promise<boolean> {
  const result = await cliJson<Pick<AppPrefsJson, "notifyNewMail">>(["pref", "set", "--notify-new-mail", String(enabled)]);
  return result.notifyNewMail;
}

export async function openPendingNotificationMail(): Promise<void> {
  return invoke("open_pending_notification_mail");
}

// ── OAuth2（设备码授权）──
export async function oauthBeginDevice(provider: OAuthProvider, clientId?: string): Promise<DeviceFlowStart> {
  return invoke("oauth_begin_device", { provider, clientId: clientId ?? null });
}

export async function oauthPollDevice(
  provider: OAuthProvider,
  clientId: string,
  clientSecret: string | null,
  deviceCode: string,
): Promise<DevicePoll> {
  return invoke("oauth_poll_device", { provider, clientId, clientSecret, deviceCode });
}

export async function oauthBeginBrowser(
  provider: OAuthProvider,
  clientId: string,
  clientSecret?: string,
  loginHint?: string,
): Promise<BrowserFlowStart> {
  return invoke("oauth_begin_browser", { provider, clientId, clientSecret: clientSecret ?? null, loginHint: loginHint ?? null });
}

export async function oauthFinishBrowser(flowId: string): Promise<OAuthTokens> {
  return invoke("oauth_finish_browser", { flowId });
}

// ── 账户 ──
export async function testConnection(account: Account, secret: AccountSecret): Promise<void> {
  await cliJson(["account", "test-json"], { account, secret });
}

export async function addAccount(account: Account, secret: AccountSecret): Promise<Account> {
  return cliJson(["account", "add-json"], { account, secret });
}

export async function removeAccount(accountId: string): Promise<void> {
  await cliJson(["account", "remove", "--id", accountId]);
}

// ── 目录 ──
export async function listFolders(accountId: string): Promise<FolderInfo[]> {
  return cliJson(["folders", "--account", accountId]);
}

export async function createFolder(accountId: string, name: string): Promise<void> {
  await cliJson(["folder", "create", "--account", accountId, "--folder", name]);
}

export async function deleteFolder(accountId: string, name: string): Promise<void> {
  await cliJson(["folder", "delete", "--account", accountId, "--folder", name]);
}

// ── 邮件 ──
export interface CachedList {
  metas: EmailMeta[];
  total: number;
}

export interface SyncResult {
  added: number;
  total: number;
}

/** 本地缓存分页读取（秒出、可离线） */
export async function listCached(accountId: string, folder: string, offset: number, limit: number): Promise<CachedList> {
  return cliJson(["list", "--account", accountId, "--folder", folder, "--offset", offset, "--limit", limit]);
}

/** 与服务器增量同步（只下载新邮件 + 回扫已读/星标/删除） */
export async function syncMessages(accountId: string, folder: string): Promise<SyncResult> {
  return cliJson(["sync", "--account", accountId, "--folder", folder]);
}

/** 按需回填更早邮件（用户继续向下翻页时触发） */
export async function syncOlderMessages(accountId: string, folder: string): Promise<SyncResult> {
  return cliJson(["sync-older", "--account", accountId, "--folder", folder]);
}

export async function setFlagged(accountId: string, folder: string, uid: number, flagged: boolean): Promise<void> {
  await cliJson(["flag", "--account", accountId, "--folder", folder, "--uid", uid, "--flagged", String(flagged)]);
}

export async function getMessage(accountId: string, folder: string, uid: number): Promise<EmailFull> {
  return cliJson(["read", "--account", accountId, "--folder", folder, "--uid", uid]);
}

export async function listThread(accountId: string, folder: string, threadId: string): Promise<EmailMeta[]> {
  return cliJson(["thread", "--account", accountId, "--folder", folder, "--thread", threadId]);
}

export async function moveMessage(accountId: string, folder: string, uid: number, target: string): Promise<void> {
  await cliJson(["move", "--account", accountId, "--folder", folder, "--uid", uid, "--target", target]);
}

export async function archiveMessage(accountId: string, folder: string, uid: number): Promise<void> {
  await cliJson(["archive", "--account", accountId, "--folder", folder, "--uid", uid]);
}

export async function setRead(accountId: string, folder: string, uid: number, read: boolean): Promise<void> {
  await cliJson(["mark", "--account", accountId, "--folder", folder, "--uid", uid, "--read", String(read)]);
}

export async function markRead(accountId: string, folder: string, uids: number[], read = true): Promise<void> {
  await cliJson(["mark", "--account", accountId, "--folder", folder, "--uids", uids.join(","), "--read", String(read)]);
}

export async function deleteMessage(
  accountId: string,
  folder: string,
  uid: number,
  permanent = false
): Promise<void> {
  const args = ["delete", "--account", accountId, "--folder", folder, "--uid", String(uid)];
  if (permanent) args.push("--permanent");
  await cliJson(args);
}

export async function sendMail(
  accountId: string,
  to: string[],
  cc: string[],
  subject: string,
  body: string,
  sign: boolean,
  attachments: string[] = []
): Promise<SendResult> {
  const args = ["send", "--account", accountId, "--to", to.join(","), "--subject", subject, "--body", body];
  if (cc.length > 0) args.push("--cc", cc.join(","));
  if (!sign) args.push("--no-sign");
  for (const path of attachments) args.push("--attach", path);
  return cliJson(args);
}

export async function saveAttachment(
  accountId: string,
  folder: string,
  uid: number,
  index: number,
  path: string
): Promise<void> {
  await cliJson(["attachment", "save", "--account", accountId, "--folder", folder, "--uid", uid, "--index", index, "--path", path]);
}

// ── 联系人（自动补全）──
export async function listContacts(query?: string): Promise<Contact[]> {
  const args = ["contacts"];
  pushFlag(args, "--query", query);
  return cliJson(args);
}

// ── 草稿 ──
export async function listDrafts(): Promise<Draft[]> {
  return cliJson(["drafts"]);
}

export async function saveDraft(draft: Draft): Promise<Draft> {
  const args = ["draft", "save", "--account", draft.accountId, "--subject", draft.subject, "--body", draft.body];
  pushFlag(args, "--id", draft.id);
  pushFlag(args, "--to", draft.to);
  pushFlag(args, "--cc", draft.cc);
  if (!draft.sign) args.push("--no-sign");
  return cliJson(args);
}

export async function deleteDraft(id: string): Promise<void> {
  await cliJson(["draft", "delete", "--id", id]);
}

// ── 过滤规则 ──
export async function saveFilter(rule: FilterRule): Promise<FilterRule[]> {
  const args = [
    "filter",
    "save",
    "--name",
    rule.name,
    "--field",
    rule.field,
    "--op",
    rule.op,
    "--value",
    rule.value,
    "--target",
    rule.targetFolder,
    "--mark-read",
    String(rule.markRead),
    "--enabled",
    String(rule.enabled),
  ];
  pushFlag(args, "--id", rule.id);
  pushFlag(args, "--account", rule.accountId);
  return cliJson(args);
}

export async function deleteFilter(id: string): Promise<FilterRule[]> {
  return cliJson(["filter", "delete", "--id", id]);
}

export async function applyFilters(accountId: string): Promise<ApplyResult> {
  return cliJson(["filter", "apply", "--account", accountId]);
}

// ── 可信联系人 ──
export async function trustSender(
  name: string,
  email: string,
  fingerprint: string,
  org?: string
): Promise<TrustedContact[]> {
  const args = ["trust", "add", "--name", name, "--email", email, "--fingerprint", fingerprint];
  pushFlag(args, "--org", org);
  return cliJson(args);
}

export async function removeTrusted(email: string): Promise<TrustedContact[]> {
  return cliJson(["trust", "remove", "--email", email]);
}
