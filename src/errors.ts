import { t } from "./i18n";

export type AppErrorKind = "auth" | "network" | "server" | "unknown";

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

/** 把后端/网络原始错误归类成用户可理解的提示。 */
export function classifyError(e: unknown): AppError {
  const raw = rawOf(e).trim();
  const line = raw.split("\n").map((s) => s.trim()).find(Boolean) ?? raw;

  if (
    /OAuth2|重新授权|授权已失效|授权可能已失效|AUTHENTICATE|authentication failed|login failed|认证被拒|请重新授权/i.test(
      line
    )
  ) {
    return {
      kind: "auth",
      message: t("登录已失效，请重新授权此账户"),
      raw,
    };
  }

  if (
    /无法连接|Connection refused|timed out|timeout|网络|network|Name or service not known|DNS|TLS 初始化|broken pipe|connection reset/i.test(
      line
    )
  ) {
    return {
      kind: "network",
      message: t("网络连接失败，请检查网络后重试"),
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
  if (base.kind === "auth" || base.kind === "network") {
    return { ...base, message: prefix + base.message };
  }
  return { ...base, message: prefix + base.message };
}
