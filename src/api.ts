import { invoke } from "@tauri-apps/api/core";
import type {
  Account,
  AccountSecret,
  AppStateView,
  ApplyResult,
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
  SendResult,
  TrustedContact,
} from "./types";

export async function getState(): Promise<AppStateView> {
  return invoke<AppStateView>("get_state");
}

// ── 身份 / Ledger ──
export async function ledgerGetAddresses(count = 5): Promise<LedgerAccountRow[]> {
  return invoke("ledger_get_addresses", { count });
}

export async function bindLedger(path: string, address: string): Promise<IdentityInfo> {
  return invoke("bind_ledger", { path, address });
}

export async function useLocalKey(): Promise<IdentityInfo> {
  return invoke("use_local_key");
}

// ── 偏好 ──
export async function getCloseBehavior(): Promise<"hide" | "quit"> {
  return invoke("get_close_behavior");
}

export async function setCloseBehavior(behavior: "hide" | "quit"): Promise<"hide" | "quit"> {
  return invoke("set_close_behavior", { behavior });
}

// ── OAuth2（Microsoft 设备码授权）──
export async function oauthBeginDevice(clientId?: string): Promise<DeviceFlowStart> {
  return invoke("oauth_begin_device", { clientId: clientId ?? null });
}

export async function oauthPollDevice(clientId: string, deviceCode: string): Promise<DevicePoll> {
  return invoke("oauth_poll_device", { clientId, deviceCode });
}

// ── 账户 ──
export async function testConnection(account: Account, secret: AccountSecret): Promise<void> {
  return invoke("test_connection", { account, secret });
}

export async function addAccount(account: Account, secret: AccountSecret): Promise<Account> {
  return invoke("add_account", { account, secret });
}

export async function removeAccount(accountId: string): Promise<void> {
  return invoke("remove_account", { accountId });
}

// ── 目录 ──
export async function listFolders(accountId: string): Promise<FolderInfo[]> {
  return invoke("list_folders", { accountId });
}

export async function createFolder(accountId: string, name: string): Promise<void> {
  return invoke("create_folder", { accountId, name });
}

// ── 邮件 ──
export async function fetchMessages(accountId: string, folder: string, limit = 30): Promise<EmailMeta[]> {
  return invoke("fetch_messages", { accountId, folder, limit });
}

export async function getMessage(accountId: string, folder: string, uid: number): Promise<EmailFull> {
  return invoke("get_message", { accountId, folder, uid });
}

export async function moveMessage(accountId: string, folder: string, uid: number, target: string): Promise<void> {
  return invoke("move_message", { accountId, folder, uid, target });
}

export async function setRead(accountId: string, folder: string, uid: number, read: boolean): Promise<void> {
  return invoke("set_read", { accountId, folder, uid, read });
}

export async function markRead(accountId: string, folder: string, uids: number[], read = true): Promise<void> {
  return invoke("mark_read", { accountId, folder, uids, read });
}

export async function deleteMessage(
  accountId: string,
  folder: string,
  uid: number,
  permanent = false
): Promise<void> {
  return invoke("delete_message", { accountId, folder, uid, permanent });
}

export async function sendMail(
  accountId: string,
  to: string[],
  cc: string[],
  subject: string,
  body: string,
  sign: boolean
): Promise<SendResult> {
  return invoke("send_mail", { accountId, to, cc, subject, body, sign });
}

// ── 联系人（自动补全）──
export async function listContacts(query?: string): Promise<Contact[]> {
  return invoke("list_contacts", { query: query ?? null });
}

// ── 草稿 ──
export async function listDrafts(): Promise<Draft[]> {
  return invoke("list_drafts");
}

export async function saveDraft(draft: Draft): Promise<Draft> {
  return invoke("save_draft", { draft });
}

export async function deleteDraft(id: string): Promise<void> {
  return invoke("delete_draft", { id });
}

// ── 过滤规则 ──
export async function saveFilter(rule: FilterRule): Promise<FilterRule[]> {
  return invoke("save_filter", { rule });
}

export async function deleteFilter(id: string): Promise<FilterRule[]> {
  return invoke("delete_filter", { id });
}

export async function applyFilters(accountId: string): Promise<ApplyResult> {
  return invoke("apply_filters", { accountId });
}

// ── 可信联系人 ──
export async function trustSender(
  name: string,
  email: string,
  fingerprint: string,
  org?: string
): Promise<TrustedContact[]> {
  return invoke("trust_sender", { name, email, fingerprint, org: org ?? null });
}

export async function removeTrusted(email: string): Promise<TrustedContact[]> {
  return invoke("remove_trusted", { email });
}
