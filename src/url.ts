import { t } from "./i18n";
import { invoke } from "@tauri-apps/api/core";
import { ask } from "@tauri-apps/plugin-dialog";

interface OpenExternalOptions {
  label?: string | null;
  confirm?: boolean;
}

export function normalizeExternalUrl(raw: string | null | undefined): string | null {
  const value = raw?.trim();
  if (!value) return null;
  if (/^https?:\/\//i.test(value)) return value;
  if (/^\/\//.test(value)) return `https:${value}`;
  if (/^www\./i.test(value)) return `https://${value}`;
  return null;
}

function hostOf(url: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return "";
  }
}

function domainLikeText(text: string | null | undefined): string | null {
  const value = text?.trim();
  if (!value) return null;
  const match = value.match(/(?:https?:\/\/)?(?:www\.)?([a-z0-9-]+(?:\.[a-z0-9-]+)+)/i);
  return match?.[1]?.toLowerCase() ?? null;
}

/**
 * 正常链接直接交系统浏览器打开；只有链接文字看着是 A 域名、实际指向 B 域名
 * （典型钓鱼手法）时才弹确认。注意 WKWebView 里 window.confirm 是 no-op
 * （静默返回 false），确认框必须走 tauri-plugin-dialog。
 */
async function shouldOpenExternal(url: string, label?: string | null): Promise<boolean> {
  const host = hostOf(url);
  const shownDomain = domainLikeText(label);
  const mismatch = shownDomain && host && shownDomain !== host && !host.endsWith(`.${shownDomain}`);
  if (!mismatch) return true;
  return ask(
    t("链接文字看起来是 {shown}，实际指向的是 {host}。", { shown: shownDomain, host }) + `\n\n${url}\n\n` + t("仍要用系统浏览器打开吗？"),
    { title: t("链接地址与显示不符"), kind: "warning", okLabel: t("打开"), cancelLabel: t("取消") }
  );
}

export async function openExternalUrl(raw: string | null | undefined, options: OpenExternalOptions = {}) {
  const url = normalizeExternalUrl(raw);
  if (!url) return;
  if (options.confirm !== false && !(await shouldOpenExternal(url, options.label))) return;
  await invoke("open_external_url", { url });
}
