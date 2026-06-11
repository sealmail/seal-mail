export type TrustTag = "verified" | "signedUnknown" | "unsigned" | "tampered" | "impersonation";

export interface Account {
  id: string;
  label: string;
  email: string;
  displayName: string;
  protocol: "imap" | "pop3";
  incomingHost: string;
  incomingPort: number;
  smtpHost: string;
  smtpPort: number;
  smtpSecurity: "ssl" | "starttls";
  username: string;
  /** password | oauth2 */
  auth: "password" | "oauth2";
}

export interface OAuthTokens {
  accessToken: string;
  refreshToken: string;
  expiresAt: number;
  clientId: string;
}

export interface DeviceFlowStart {
  userCode: string;
  verificationUri: string;
  message: string;
  deviceCode: string;
  /** 轮询间隔（秒） */
  interval: number;
  expiresIn: number;
  clientId: string;
}

export type DevicePoll = { status: "pending" } | { status: "ok"; tokens: OAuthTokens };

export interface AccountSecret {
  password: string;
  smtpPassword?: string | null;
  oauth?: OAuthTokens | null;
}

export interface TrustedContact {
  name: string;
  email: string;
  fingerprint: string;
  org?: string | null;
  since: string;
  verifiedCount: number;
}

export interface FilterRule {
  id: string;
  name: string;
  accountId?: string | null;
  field: "from" | "to" | "subject" | "body";
  op: "contains" | "not_contains" | "equals" | "starts_with" | "ends_with";
  value: string;
  targetFolder: string;
  markRead: boolean;
  enabled: boolean;
}

export interface RiskInfo {
  kind: "fund" | "account" | "contract";
  reasons: string[];
}

export type VerifyDetail =
  | {
      status: "verified";
      fingerprint: string;
      method: string;
      contactName: string;
      since: string;
      verifiedCount: number;
    }
  | { status: "signedUnknown"; fingerprint: string; method: string }
  | { status: "unsigned" }
  | { status: "tampered"; signedHash: string; gotHash: string; fingerprint: string; method: string }
  | {
      status: "impersonation";
      claimed: string;
      gotFingerprint?: string | null;
      realFingerprint: string;
      gotDomain: string;
      realDomain: string;
    };

export interface AttachmentMeta {
  name: string;
  size: number;
  mime: string;
}

export interface EmailMeta {
  uid: number;
  accountId: string;
  folder: string;
  fromName: string;
  fromAddr: string;
  subject: string;
  preview: string;
  dateDisplay: string;
  timestamp: number;
  unread: boolean;
  flagged: boolean;
  lang: string;
  trust: TrustTag;
  risk?: RiskInfo | null;
  hasAttach: boolean;
}

export interface EmailFull {
  meta: EmailMeta;
  to: string[];
  cc: string[];
  bodyText: string;
  bodyHtml?: string | null;
  attachments: AttachmentMeta[];
  verify: VerifyDetail;
}

export interface IdentityInfo {
  fingerprint: string;
  publicKey: string;
  created: string;
  /** local | ledger */
  mode: string;
  ledgerPath?: string | null;
  ledgerAddress?: string | null;
}

export interface LedgerAccountRow {
  index: number;
  path: string;
  address: string;
}

/** 写信草稿（本地保存，发送成功后删除） */
export interface Draft {
  id: string;
  accountId: string;
  to: string;
  cc: string;
  subject: string;
  body: string;
  sign: boolean;
  updatedAt: number;
}

/** 自动收集的联系人（写信补全用） */
export interface Contact {
  name: string;
  email: string;
  lastSeen: number;
  count: number;
}

export interface FolderInfo {
  name: string;
  display: string;
  /** 特殊目录角色："trash" 等 */
  role?: string;
}

export interface AppStateView {
  accounts: Account[];
  identity: IdentityInfo;
  trusted: TrustedContact[];
  filters: FilterRule[];
  localFolders: string[];
}

export interface SendResult {
  signed: boolean;
  method: string;
  fingerprint: string;
  shortFingerprint: string;
  sentAt: string;
}

export interface ApplyResult {
  moved: number;
  details: string[];
}

export interface ProviderPreset {
  key: string;
  label: string;
  note?: string;
  /** 支持 Microsoft OAuth2 设备码授权（Exchange Online / Outlook.com 已强制） */
  oauth?: boolean;
  protocol: "imap" | "pop3";
  incomingHost: string;
  incomingPort: number;
  smtpHost: string;
  smtpPort: number;
  smtpSecurity: "ssl" | "starttls";
}

export const PROVIDER_PRESETS: ProviderPreset[] = [
  {
    key: "exchange-online",
    label: "Exchange Online / Outlook · Office 365",
    note: "微软已停用 IMAP/SMTP 密码登录，请使用 OAuth2 授权（点击下方按钮用 Microsoft 账户登录）",
    oauth: true,
    protocol: "imap",
    incomingHost: "outlook.office365.com",
    incomingPort: 993,
    smtpHost: "smtp.office365.com",
    smtpPort: 587,
    smtpSecurity: "starttls",
  },
  {
    key: "exchange-onprem",
    label: "Exchange Server（自建 / 本地部署）",
    note: "填写公司 Exchange 的 IMAP/SMTP 地址（管理员需启用 IMAP 服务）",
    protocol: "imap",
    incomingHost: "mail.example.com",
    incomingPort: 993,
    smtpHost: "mail.example.com",
    smtpPort: 587,
    smtpSecurity: "starttls",
  },
  {
    key: "gmail",
    label: "Gmail / Google Workspace",
    note: "需开启两步验证并使用应用专用密码",
    protocol: "imap",
    incomingHost: "imap.gmail.com",
    incomingPort: 993,
    smtpHost: "smtp.gmail.com",
    smtpPort: 465,
    smtpSecurity: "ssl",
  },
  {
    key: "icloud",
    label: "iCloud Mail",
    note: "需使用 App 专用密码",
    protocol: "imap",
    incomingHost: "imap.mail.me.com",
    incomingPort: 993,
    smtpHost: "smtp.mail.me.com",
    smtpPort: 587,
    smtpSecurity: "starttls",
  },
  {
    key: "qq",
    label: "QQ 邮箱",
    note: "需在设置中开启 IMAP 并使用授权码",
    protocol: "imap",
    incomingHost: "imap.qq.com",
    incomingPort: 993,
    smtpHost: "smtp.qq.com",
    smtpPort: 465,
    smtpSecurity: "ssl",
  },
  {
    key: "163",
    label: "网易 163 邮箱",
    note: "需开启 IMAP 并使用授权码",
    protocol: "imap",
    incomingHost: "imap.163.com",
    incomingPort: 993,
    smtpHost: "smtp.163.com",
    smtpPort: 465,
    smtpSecurity: "ssl",
  },
  {
    key: "custom-imap",
    label: "其他邮箱（IMAP）",
    protocol: "imap",
    incomingHost: "",
    incomingPort: 993,
    smtpHost: "",
    smtpPort: 465,
    smtpSecurity: "ssl",
  },
  {
    key: "custom-pop3",
    label: "其他邮箱（POP3）",
    note: "POP3 无服务器目录，SealMail 会用本地目录管理归类",
    protocol: "pop3",
    incomingHost: "",
    incomingPort: 995,
    smtpHost: "",
    smtpPort: 465,
    smtpSecurity: "ssl",
  },
];
