// 外观主题：data-theme 挂在 <html> 上，styles.css 按 html[data-theme="dark"] 覆盖变量。
// "system" 跟随 OS：挂载 matchMedia change 监听，运行中切换系统明暗也实时生效。
import { useEffect } from "react";
import { getThemePref, type ThemePref } from "./api";

let currentPref: ThemePref = "system";

function darkQuery() {
  return window.matchMedia("(prefers-color-scheme: dark)");
}

/** 应用主题偏好并记住它（供 OS 明暗切换时按 system 重算） */
export function applyTheme(pref: ThemePref) {
  currentPref = pref;
  const dark = pref === "dark" || (pref === "system" && darkQuery().matches);
  document.documentElement.setAttribute("data-theme", dark ? "dark" : "light");
}

/**
 * App 根（主窗口 / 邮件子窗口）挂载：读取主题偏好并应用；
 * pref 为 system 时跟随 OS 明暗实时切换。
 */
export function useTheme() {
  useEffect(() => {
    getThemePref()
      .then(applyTheme)
      .catch((e) => console.error("读取主题偏好失败", e));
    const mq = darkQuery();
    const onChange = () => {
      if (currentPref === "system") applyTheme("system");
    };
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);
}
