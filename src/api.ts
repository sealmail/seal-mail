import { invoke } from "@tauri-apps/api/core";
import * as demo from "./demo";
import type {
  Account,
  AccountSecret,
  AppStateView,
  ApplyResult,
  EmailFull,
  EmailMeta,
  FilterRule,
  FolderInfo,
  SendResult,
  TrustedContact,
} from "./types";

export const isDemo = (accountId: string) => accountId === demo.DEMO_ACCOUNT_ID;

export async function getState(): Promise<AppStateView> {
  return invoke<AppStateView>("get_state");
}

export async function testConnection(account: Account, secret: AccountSecret): Promise<void> {
  return invoke("test_connection", { account, secret });
}

export async function addAccount(account: Account, secret: AccountSecret): Promise<Account> {
  return invoke("add_account", { account, secret });
}

export async function removeAccount(accountId: string): Promise<void> {
  return invoke("remove_account", { accountId });
}

export async function listFolders(accountId: string): Promise<FolderInfo[]> {
  if (isDemo(accountId)) return demo.demoFolders();
  return invoke("list_folders", { accountId });
}

export async function createFolder(accountId: string, name: string): Promise<void> {
  if (isDemo(accountId)) return demo.demoCreateFolder(name);
  return invoke("create_folder", { accountId, name });
}

export async function fetchMessages(accountId: string, folder: string, limit = 30): Promise<EmailMeta[]> {
  if (isDemo(accountId)) return demo.demoFetch(folder).map((f) => f.meta);
  return invoke("fetch_messages", { accountId, folder, limit });
}

export async function getMessage(accountId: string, folder: string, uid: number): Promise<EmailFull> {
  if (isDemo(accountId)) {
    const m = demo.demoGet(uid);
    if (!m) throw new Error("演示邮件不存在");
    return m;
  }
  return invoke("get_message", { accountId, folder, uid });
}

export async function moveMessage(accountId: string, folder: string, uid: number, target: string): Promise<void> {
  if (isDemo(accountId)) return demo.demoMove(uid, target);
  return invoke("move_message", { accountId, folder, uid, target });
}

export async function setRead(accountId: string, folder: string, uid: number, read: boolean): Promise<void> {
  if (isDemo(accountId)) return demo.demoSetRead(uid, read);
  return invoke("set_read", { accountId, folder, uid, read });
}

export async function deleteMessage(accountId: string, folder: string, uid: number): Promise<void> {
  if (isDemo(accountId)) return demo.demoMove(uid, "__deleted__");
  return invoke("delete_message", { accountId, folder, uid });
}

export async function sendMail(
  accountId: string,
  to: string[],
  cc: string[],
  subject: string,
  body: string,
  sign: boolean
): Promise<SendResult> {
  if (isDemo(accountId)) {
    await new Promise((r) => setTimeout(r, 600));
    return {
      signed: sign,
      fingerprint: "演示模式 · 未真正发送",
      shortFingerprint: "DEMO…MODE",
      sentAt: new Date().toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" }),
    };
  }
  return invoke("send_mail", { accountId, to, cc, subject, body, sign });
}

export async function saveFilter(rule: FilterRule): Promise<FilterRule[]> {
  return invoke("save_filter", { rule });
}

export async function deleteFilter(id: string): Promise<FilterRule[]> {
  return invoke("delete_filter", { id });
}

export async function applyFilters(accountId: string): Promise<ApplyResult> {
  if (isDemo(accountId)) return { moved: 0, details: ["演示模式下过滤规则不执行"] };
  return invoke("apply_filters", { accountId });
}

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
