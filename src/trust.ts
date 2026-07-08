import { t } from "./i18n";
import type { EmailFull, RiskInfo, TrustTag, VerifyDetail } from "./types";

export const TRUST_LABEL: Record<TrustTag, string> = {
  verified: "已验证",
  signedUnknown: "签名有效",
  unsigned: "未盖印",
  tampered: "封印破损",
  impersonation: "冒充身份",
};

export function statusText(v: VerifyDetail): { title: string; sub: string; tone: "jade" | "gold" | "gray" | "red"; railBg: string } {
  switch (v.status) {
    case "verified":
      return {
        title: t("已验证本人"),
        sub: t("数字签名有效 · 发件人身份与可信记录完全匹配"),
        tone: "jade",
        railBg: "#F6F8F5",
      };
    case "signedUnknown":
      return {
        title: t("签名有效 · 尚未列入可信"),
        sub: t("签名校验通过，但这把密钥还不在你的可信联系人记录中。确认对方身份后可加入可信。"),
        tone: "gold",
        railBg: "#F0F4EE",
      };
    case "unsigned":
      return {
        title: t("未盖印 · 身份未知"),
        sub: t("该发件人未签名，无法验证其真实身份。"),
        tone: "gray",
        railBg: "#F4F6F3",
      };
    case "tampered":
      return {
        title: t("封印破损 · 内容被改动"),
        sub: t("签名存在，但邮件正文在传输中被修改。请勿信任此内容。"),
        tone: "red",
        railBg: "#FCF4F2",
      };
    case "impersonation":
      return {
        title: t("冒充已知联系人"),
        sub: t("显示名与可信联系人相同，但密钥指纹与域名均不符。疑似钓鱼。"),
        tone: "red",
        railBg: "#FCF1EF",
      };
  }
}

export const TONE_COLOR: Record<string, string> = {
  jade: "#1E6B49",
  gold: "#5F695F",
  gray: "#626A62",
  red: "#9A2C1D",
};

export interface CheckRow {
  kind: "ok" | "bad" | "warn" | "neu";
  label: string;
  val: string;
  mono?: boolean;
  sub?: string;
}

export function buildChecks(mail: EmailFull): CheckRow[] {
  const v = mail.verify;
  switch (v.status) {
    case "verified":
      return [
        { kind: "ok", label: t("发件人身份"), val: v.contactName, sub: mail.meta.fromAddr },
        { kind: "ok", label: t("签名方式"), val: v.method },
        { kind: "ok", label: t("密钥指纹"), val: v.fingerprint, mono: true, sub: t("与可信记录一致") },
        { kind: "ok", label: t("内容完整性"), val: t("正文与签名哈希一致 · 未被改动") },
        { kind: "ok", label: t("密钥历史"), val: t("自 {since} 起 · 已验证 {n} 封", { since: v.since, n: v.verifiedCount }) },
      ];
    case "signedUnknown":
      return [
        { kind: "ok", label: t("签名校验"), val: t("签名有效 · 内容未被改动") },
        { kind: "ok", label: t("签名方式"), val: v.method },
        { kind: "warn", label: t("密钥指纹"), val: v.fingerprint, mono: true, sub: t("首次见到这把密钥") },
        { kind: "neu", label: t("建议"), val: t("通过其他渠道核实对方身份后，将其加入可信联系人") },
      ];
    case "unsigned":
      return [
        { kind: "neu", label: t("签名"), val: t("无签名") },
        { kind: "neu", label: t("发件人身份"), val: t("无法验证") },
        { kind: "neu", label: t("与你的关系"), val: t("不在可信联系人中") },
        { kind: "neu", label: t("建议"), val: t("按常规谨慎对待；勿据此执行敏感操作") },
      ];
    case "tampered":
      return [
        { kind: "ok", label: t("签名存在"), val: v.method },
        { kind: "bad", label: t("内容完整性"), val: t("正文哈希与签名不符 — 内容在传输中被改动") },
        { kind: "neu", label: t("签名时哈希"), val: v.signedHash, mono: true },
        { kind: "bad", label: t("收到时哈希"), val: v.gotHash, mono: true, sub: t("不匹配") },
        { kind: "warn", label: t("结论"), val: t("签名无效，请勿信任此版本内容") },
      ];
    case "impersonation":
      return [
        { kind: "warn", label: t("声称身份"), val: v.claimed },
        { kind: "bad", label: t("实际域名"), val: v.gotDomain, mono: true, sub: t("可信记录为 {domain}", { domain: v.realDomain }) },
        {
          kind: "bad",
          label: t("密钥指纹"),
          val: v.gotFingerprint ?? t("未签名 / 无可信密钥"),
          mono: true,
          sub: t("与可信记录不符"),
        },
        { kind: "neu", label: t("可信记录指纹"), val: v.realFingerprint, mono: true },
        { kind: "bad", label: t("结论"), val: t("冒充已知联系人") },
      ];
  }
}

export interface BannerSpec {
  cls: "amber" | "red" | "red-strong";
  icon: string;
  title: string;
  msg: string;
  btn: string;
  solid: string;
}

export function riskBanner(mail: EmailFull): BannerSpec | null {
  const trust = mail.meta.trust;
  const risk: RiskInfo | null | undefined = mail.meta.risk;
  if (trust === "impersonation") {
    return {
      cls: "red-strong",
      icon: "⛔",
      title: t("账号安全警告 · 疑似钓鱼"),
      msg: t("此邮件冒充你的可信联系人。任何合法机构都不会索取助记词、私钥或密码。请勿点击其中链接或回复。"),
      btn: t("查看风险详情"),
      solid: "#9a2f27",
    };
  }
  if (trust === "tampered") {
    return {
      cls: "red",
      icon: "⚠",
      title: t("内容在传输中被改动"),
      msg: t("这封邮件的签名无法覆盖当前内容。执行任何操作前请向对方核实原始内容。"),
      btn: t("查看哈希对比"),
      solid: "#9a2f27",
    };
  }
  if (!risk) return null;
  if (risk.kind === "fund") {
    return {
      cls: "amber",
      icon: "🔺",
      title: t("高风险资金操作"),
      msg: t("已验证 ≠ 应当照做——付款类操作不应仅凭一封邮件执行。请通过电话或线下渠道独立核实。"),
      btn: t("查看风险详情"),
      solid: "#5f554c",
    };
  }
  if (risk.kind === "account") {
    return {
      cls: "red",
      icon: "⛔",
      title: t("账号安全风险"),
      msg: t("此邮件涉及凭据 / 密钥类敏感信息。任何合法机构都不会通过邮件索取助记词或密码。"),
      btn: t("查看风险详情"),
      solid: "#9a2f27",
    };
  }
  return {
    cls: "amber",
    icon: "⚠",
    title: t("合同 / 条款相关 · 带时限要求"),
    msg: t("涉及合同条款且带有时限措辞。签署前请确认条款与此前沟通一致。"),
    btn: t("查看风险详情"),
    solid: "#5f554c",
  };
}

export function shortFpr(fpr: string): string {
  const g = fpr.split(" ");
  return g.length >= 2 ? `${g[0]}…${g[g.length - 1]}` : fpr;
}
