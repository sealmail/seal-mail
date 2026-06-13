import { openUrl } from "@tauri-apps/plugin-opener";

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

function shouldOpenExternal(url: string, label?: string | null): boolean {
  const host = hostOf(url);
  const shownDomain = domainLikeText(label);
  const mismatch = shownDomain && host && shownDomain !== host && !host.endsWith(`.${shownDomain}`);
  const lines = [`即将用系统默认浏览器打开：`, url];
  if (mismatch) {
    lines.push("", `注意：链接文字看起来是 ${shownDomain}，实际打开的是 ${host}。`);
  }
  return window.confirm(lines.join("\n"));
}

export async function openExternalUrl(raw: string | null | undefined, options: OpenExternalOptions = {}) {
  const url = normalizeExternalUrl(raw);
  if (!url) return;
  if (options.confirm !== false && !shouldOpenExternal(url, options.label)) return;
  await openUrl(url);
}
