import { describe, expect, test } from "bun:test";
import { applyLangPref } from "./i18n";
import { classifyError, classifyErrorWithPrefix } from "./errors";

describe("classifyError", () => {
  test("maps OAuth/auth failures to auth kind with actionable message", () => {
    applyLangPref("zh");
    const e = classifyError(
      "IMAP OAuth2 登录失败（授权可能已失效，请重新授权）: No Response: AUTHENTICATE failed."
    );
    expect(e.kind).toBe("auth");
    expect(e.message).toContain("重新授权");
    expect(e.raw).toContain("OAuth2");
  });

  test("maps connection failures to network kind", () => {
    applyLangPref("en");
    const e = classifyError("无法连接 imap.gmail.com:993 — Connection refused (os error 61)");
    expect(e.kind).toBe("network");
    expect(e.message.toLowerCase()).toContain("network");
  });

  test("prefix is applied for sync failures", () => {
    applyLangPref("zh");
    const e = classifyErrorWithPrefix(new Error("database is locked"), "同步失败（本地缓存仍可用）：");
    expect(e.kind).toBe("server");
    expect(e.message.startsWith("同步失败")).toBe(true);
  });
});
