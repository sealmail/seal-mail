// 界面多语言：gettext 风格——中文原文即 key，t() 在英文环境查词典、缺条目回退中文。
// 语言偏好存 prefs.language（system/zh/en），"system" 按 navigator.language 解析。
// 组件里用 useI18n() 拿 t（订阅语言切换触发重渲）；模块级/非组件代码直接 import { t }。
import { useEffect, useReducer } from "react";
import { EN } from "./i18n.en";

export type Lang = "zh" | "en";
export type LangPref = "system" | Lang;

let current: Lang = "zh";
const listeners = new Set<() => void>();

export function resolveLang(pref: LangPref): Lang {
  if (pref === "zh" || pref === "en") return pref;
  return (navigator.language || "zh").toLowerCase().startsWith("zh") ? "zh" : "en";
}

export function getLang(): Lang {
  return current;
}

export function applyLangPref(pref: LangPref) {
  const next = resolveLang(pref);
  if (next === current) return;
  current = next;
  listeners.forEach((fn) => fn());
}

/** 翻译一条界面文案；vars 替换 {name} 占位符 */
export function t(zh: string, vars?: Record<string, string | number>): string {
  let s = current === "zh" ? zh : (EN[zh] ?? zh);
  if (vars) {
    for (const [k, v] of Object.entries(vars)) s = s.split(`{${k}}`).join(String(v));
  }
  return s;
}

/** 订阅语言切换的组件入口：语言变化时触发重渲，返回 t */
export function useI18n() {
  const [, force] = useReducer((x: number) => x + 1, 0);
  useEffect(() => {
    listeners.add(force);
    return () => {
      listeners.delete(force);
    };
  }, []);
  return t;
}
