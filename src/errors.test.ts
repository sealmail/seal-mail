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

  test("network failure during OAuth2 token refresh is network, not auth", () => {
    applyLangPref("zh");
    // 离线时刷新令牌：报错带 OAuth2 字样但根因是网络，不能把用户推去重新授权
    const e = classifyError("OAuth2 刷新失败: connection timed out");
    expect(e.kind).toBe("network");
    expect(e.message).toContain("网络");
  });

  test("connection refused during token refresh is network even with OAuth2 prefix", () => {
    applyLangPref("zh");
    const e = classifyError("请求 Google 失败: error sending request: Connection refused (os error 61)");
    expect(e.kind).toBe("network");
  });

  test("dns failure mentioning OAuth2 is network", () => {
    applyLangPref("zh");
    const e = classifyError("OAuth2 token refresh failed: dns error: no such host");
    expect(e.kind).toBe("network");
  });

  test("invalid_grant refresh rejection is auth", () => {
    applyLangPref("zh");
    const e = classifyError("OAuth2 授权已失效，请在账户设置中重新授权: invalid_grant");
    expect(e.kind).toBe("auth");
    expect(e.message).toContain("重新授权");
  });

  test("401 unauthorized is auth", () => {
    applyLangPref("zh");
    expect(classifyError("HTTP 401 Unauthorized").kind).toBe("auth");
  });

  test("auth rejection retry message is auth", () => {
    applyLangPref("zh");
    expect(
      classifyError("IMAP OAuth2 登录失败（授权可能已失效，请重新授权）: No Response: AUTHENTICATE failed.").kind
    ).toBe("auth");
    expect(classifyError("发送失败: permanent error (535): 5.7.8 Username and Password not accepted").kind).toBe("auth");
  });

  test("prefix is applied for sync failures", () => {
    applyLangPref("zh");
    const e = classifyErrorWithPrefix(new Error("database is locked"), "同步失败（本地缓存仍可用）：");
    expect(e.kind).toBe("server");
    expect(e.message.startsWith("同步失败")).toBe(true);
  });

  test("OAuth2 缺 Client ID/Secret 归为配置错误，原样展示且不推重新授权", () => {
    applyLangPref("zh");
    // 后端真实文案（src-tauri/src/oauth.rs）：初次配置时还没有任何授权可「重新」
    const e = classifyError("Gmail OAuth2 需要填写 Google Cloud OAuth Client ID/Secret");
    expect(e.kind).toBe("config");
    expect(e.message).toContain("需要填写");
    expect(e.message).not.toContain("重新授权");
  });

  test("不支持的 OAuth2 服务商归为配置错误", () => {
    applyLangPref("zh");
    const e = classifyError("不支持的 OAuth2 服务商");
    expect(e.kind).toBe("config");
    expect(e.message).toContain("不支持的 OAuth2 服务商");
  });

  test("缺少 refresh_token（offline_access）归为配置错误而非网络错误", () => {
    applyLangPref("zh");
    // 后端真实文案（src-tauri/src/oauth.rs）：scope/配置问题，不是断网
    const e = classifyError("令牌响应缺少 refresh_token（请确认 offline_access 权限）");
    expect(e.kind).toBe("config");
    expect(e.message).toContain("refresh_token");
  });

  test("offline 断网仍是网络错误（词边界收紧后不误伤）", () => {
    applyLangPref("zh");
    const e = classifyError("请求失败: network is offline");
    expect(e.kind).toBe("network");
  });
});
