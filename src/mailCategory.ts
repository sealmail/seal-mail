import type { EmailMeta } from "./types";

export type MailCategory = "all" | "personal" | "business" | "ads";
export type ConcreteMailCategory = Exclude<MailCategory, "all">;

export const CATEGORY_LABEL: Record<MailCategory, string> = {
  all: "全部",
  personal: "个人",
  business: "商务",
  ads: "广告",
};

export const CATEGORY_TAG: Record<ConcreteMailCategory, string> = {
  personal: "个人",
  business: "商务",
  ads: "广告",
};

const PERSONAL_DOMAINS = new Set([
  "gmail.com",
  "googlemail.com",
  "outlook.com",
  "hotmail.com",
  "live.com",
  "icloud.com",
  "me.com",
  "qq.com",
  "163.com",
  "126.com",
]);

const ADS_KW = [
  "unsubscribe",
  "newsletter",
  "promotion",
  "promo",
  "sale",
  "discount",
  "coupon",
  "deal",
  "offer",
  "marketing",
  "digest",
  "广告",
  "促销",
  "优惠",
  "折扣",
  "限时",
  "特惠",
  "订阅",
  "活动",
  "会员",
];

const BUSINESS_KW = [
  "meeting",
  "calendar",
  "invite",
  "invoice",
  "receipt",
  "payment",
  "contract",
  "agreement",
  "project",
  "quote",
  "order",
  "ticket",
  "support",
  "security",
  "login",
  "report",
  "商务",
  "会议",
  "日程",
  "邀请",
  "发票",
  "收据",
  "付款",
  "订单",
  "合同",
  "协议",
  "项目",
  "报价",
  "工单",
  "客服",
  "安全",
  "登录",
  "报告",
  "审批",
];

function includesAny(text: string, keywords: string[]) {
  return keywords.some((kw) => text.includes(kw));
}

export function classifyMail(m: EmailMeta): ConcreteMailCategory {
  const haystack = `${m.fromName} ${m.fromAddr} ${m.subject} ${m.preview}`.toLowerCase();
  if (includesAny(haystack, ADS_KW)) return "ads";
  if (includesAny(haystack, BUSINESS_KW)) return "business";

  const domain = m.fromAddr.split("@")[1]?.toLowerCase() ?? "";
  if (domain && !PERSONAL_DOMAINS.has(domain)) return "business";
  return "personal";
}
