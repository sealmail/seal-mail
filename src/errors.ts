import { t } from "./i18n";

export type AppErrorKind = "auth" | "config" | "network" | "server" | "unknown";

export interface AppError {
  kind: AppErrorKind;
  /** 用户可见短文案 */
  message: string;
  /** 原始错误（排查用） */
  raw: string;
}

function rawOf(e: unknown): string {
  if (e instanceof Error) return e.message || String(e);
  return String(e);
}

// 账户配置类错误（设置阶段的指引，不是「登录失效」）：
// 缺 OAuth Client ID/Secret、不支持的 OAuth2 服务商、令牌缺 refresh_token 等。
// 必须最先判断——这些文案含 OAuth2 / offline 字样，会被下面的泛认证/网络规则
// 误吞，但正确做法是原样展示后端指引，让用户补配置，而不是推去重新授权或报断网。
const CONFIG_RE =
  /需要填写|client id|client secret|不支持的 oauth2 服务商|unsupported oauth2 provider|缺少 refresh_token|missing refresh_token|offline_access/i;

// 明确的认证被拒信号（后端真实文案见 src-tauri/src/oauth.rs / imap_client.rs / pop3_client.rs）：
// invalid_grant / 401 / 535 / AUTHENTICATE failed / 授权已失效 / 认证被拒 等。
// 这些即使同一行里出现网络字样也按认证处理——它们只在服务器明确拒绝时才会出现。
const STRONG_AUTH_RE =
  /invalid_grant|invalid credentials|\b401\b|unauthorized|\(535\)|授权已失效|授权可能已失效|认证被拒|拒绝了授权|AUTHENTICATE failed|authentication failed|login failed|请重新授权/i;

// 网络类信号：断网/超时/DNS/连接被拒等。刷新 OAuth2 令牌时断网的报错
// （如「OAuth2 刷新失败: connection timed out」）必须归为网络，而不是把用户推去重新授权。
// offline 用词边界：避免误伤 offline_access（那是 scope/配置问题，归 config）。
const NETWORK_RE =
  /无法连接|Connection refused|timed out|timeout|超时|网络|network|Name or service not known|DNS|dns error|TLS 初始化|broken pipe|connection reset|unreachable|\boffline\b|离线|error sending request/i;

// 泛认证信号（如裸 OAuth2 字样）：只有在排除网络信号之后才按认证处理。
const GENERIC_AUTH_RE = /OAuth2|重新授权|AUTHENTICATE/i;

/** 把后端/网络原始错误归类成用户可理解的提示。 */
export function classifyError(e: unknown): AppError {
  const raw = rawOf(e).trim();
  const line = raw.split("\n").map((s) => s.trim()).find(Boolean) ?? raw;

  if (CONFIG_RE.test(line)) {
    // 配置错误按原文展示（后端已写好可操作的指引）；kind 不是 auth，
    // 列表/空态不会挂「重新授权」按钮（MailList errorIsAuth 只认 auth）
    const message = line.length > 280 ? `${line.slice(0, 277)}…` : line;
    return { kind: "config", message, raw };
  }

  if (STRONG_AUTH_RE.test(line)) {
    return {
      kind: "auth",
      message: t("登录已失效，请重新授权此账户"),
      raw,
    };
  }

  if (NETWORK_RE.test(line)) {
    return {
      kind: "network",
      message: t("网络连接失败，请检查网络后重试"),
      raw,
    };
  }

  if (GENERIC_AUTH_RE.test(line)) {
    return {
      kind: "auth",
      message: t("登录已失效，请重新授权此账户"),
      raw,
    };
  }

  if (/database is locked|busy|SQLITE_BUSY/i.test(line)) {
    return {
      kind: "server",
      message: t("本地缓存正忙，请稍后重试"),
      raw,
    };
  }

  // 后端已写中文用户文案时尽量原样展示（截断过长链）
  const message = line.length > 280 ? `${line.slice(0, 277)}…` : line;
  return { kind: "unknown", message, raw };
}

/** 带前缀的列表错误（如「同步失败：」）。认证类仍用统一可操作文案。 */
export function classifyErrorWithPrefix(e: unknown, prefix: string): AppError {
  const base = classifyError(e);
  return { ...base, message: prefix + base.message };
}
