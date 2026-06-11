// 演示数据：未配置真实账户时展示设计稿中的 6 封示例邮件，便于体验全部信任状态。
import type {
  AppStateView,
  EmailFull,
  EmailMeta,
  FolderInfo,
  TrustedContact,
  VerifyDetail,
} from "./types";

export const DEMO_ACCOUNT_ID = "__demo__";

const realDanielFpr = "6B41 9C72 0AD8 51FE 9C72 0AD8 6B41 51FE";

export const demoTrusted: TrustedContact[] = [
  {
    name: "Mara Castellanos",
    email: "mara@aragon.eth",
    fingerprint: "4D1A 88E0 2C9F B720 88E0 2C9F 4D1A B720",
    org: "Aragon DAO",
    since: "2025-04-02",
    verifiedCount: 128,
  },
  {
    name: "Daniel Okonkwo",
    email: "daniel@northgate.io",
    fingerprint: realDanielFpr,
    org: "Northgate Capital",
    since: "2024-06-11",
    verifiedCount: 340,
  },
  {
    name: "Lena Brandt",
    email: "l.brandt@hoffmann-recht.de",
    fingerprint: "B2D7 6610 4FAC 0C88 6610 4FAC B2D7 0C88",
    org: "Hoffmann Recht",
    since: "2025-10-12",
    verifiedCount: 46,
  },
];

interface DemoMail {
  meta: Omit<EmailMeta, "accountId" | "folder">;
  to: string[];
  bodyText: string;
  verify: VerifyDetail;
  attachments: { name: string; size: number; mime: string }[];
}

const mails: DemoMail[] = [
  {
    meta: {
      uid: 1,
      fromName: "Mara Castellanos",
      fromAddr: "mara@aragon.eth",
      subject: "Multisig 3/5 — validator key rotation (Epoch 412)",
      preview: "Posting the rotation payload for co-signing. Verify the validator pubkey before approving.",
      dateDisplay: "09:14",
      timestamp: Date.now() / 1000 - 3600,
      unread: true,
      lang: "EN",
      trust: "verified",
      risk: null,
      hasAttach: false,
    },
    to: ["aria@aragon.eth"],
    bodyText: [
      "Hi Aria,",
      "",
      "I’m posting the validator key rotation payload for Epoch 412. We need 3 of 5 treasury signers to approve before the epoch boundary at 18:00 UTC.",
      "",
      "Please verify the new validator pubkey against the registry before you co-sign — the payload hash is included in the signed envelope, so SealMail will flag any change in transit.",
      "",
      "Once you’ve confirmed, sign with your key and the multisig will broadcast automatically.",
      "",
      "— Mara",
    ].join("\n"),
    verify: {
      status: "verified",
      fingerprint: "4D1A 88E0 2C9F B720 88E0 2C9F 4D1A B720",
      method: "SealMail · Ed25519",
      contactName: "Mara Castellanos",
      since: "2025-04-02",
      verifiedCount: 128,
    },
    attachments: [],
  },
  {
    meta: {
      uid: 2,
      fromName: "Daniel Okonkwo",
      fromAddr: "daniel@northgate.io",
      subject: "Approve 250,000 USDC — new settlement address",
      preview: "Settlement account changed this morning. Please approve the transfer to the new address today.",
      dateDisplay: "08:47",
      timestamp: Date.now() / 1000 - 5400,
      unread: true,
      lang: "EN",
      trust: "verified",
      risk: {
        kind: "fund",
        reasons: [
          "收款地址首次出现：0x9C4f…A21B 从未在你的往来记录中出现过",
          "金额超过自动审批阈值：250,000 USDC 超过 50,000 的免核实上限",
          "含紧急 / 限时措辞：“before end of day”“short notice” 是常见的施压话术",
        ],
      },
      hasAttach: false,
    },
    to: ["a.wen@northgate.io"],
    bodyText: [
      "Aria,",
      "",
      "Our settlement provider rotated accounts this morning. Please approve today’s 250,000 USDC outflow to the new address: 0x9C4f…A21B.",
      "",
      "I know it’s short notice but the counterparty needs funds before end of day. Appreciate the quick turnaround.",
      "",
      "Daniel",
    ].join("\n"),
    verify: {
      status: "verified",
      fingerprint: realDanielFpr,
      method: "SealMail · Ed25519",
      contactName: "Daniel Okonkwo",
      since: "2024-06-11",
      verifiedCount: 340,
    },
    attachments: [],
  },
  {
    meta: {
      uid: 3,
      fromName: "Stibel & Partners",
      fromAddr: "legal@stibel-partners.com",
      subject: "Re: Master Services Agreement — revised clause 7",
      preview: "Updated indemnity cap as discussed in our call yesterday.",
      dateDisplay: "昨天",
      timestamp: Date.now() / 1000 - 90000,
      unread: false,
      lang: "EN",
      trust: "tampered",
      risk: {
        kind: "contract",
        reasons: ["这封含合同条款的邮件在传输中被修改，签名无法覆盖当前内容"],
      },
      hasAttach: false,
    },
    to: ["a.wen@northgate.io"],
    bodyText: [
      "Dear Aria,",
      "",
      "Please find the revised MSA. We updated the indemnity cap in clause 7 to USD 5,000,000 as discussed.",
      "",
      "Kindly counter-sign and return at your earliest convenience.",
      "",
      "Best regards,",
      "Stibel & Partners",
    ].join("\n"),
    verify: {
      status: "tampered",
      signedHash: "a3f19c…",
      gotHash: "e78022…",
      fingerprint: "7C09 1188 ED4A 9F31 1188 ED4A 7C09 9F31",
      method: "SealMail · Ed25519",
    },
    attachments: [],
  },
  {
    meta: {
      uid: 4,
      fromName: "Daniel Okonkwo",
      fromAddr: "daniel.okonkwo@northgate-finance.io",
      subject: "Urgent: confirm your wallet recovery phrase",
      preview: "Security audit requires you to re-verify your seed phrase within 2 hours.",
      dateDisplay: "07:02",
      timestamp: Date.now() / 1000 - 11000,
      unread: true,
      lang: "EN",
      trust: "impersonation",
      risk: {
        kind: "account",
        reasons: [
          "此邮件要求你提供钱包助记词。任何合法机构都不会索取助记词",
          "发件人在冒充你的可信联系人 Daniel Okonkwo",
        ],
      },
      hasAttach: false,
    },
    to: ["a.wen@northgate.io"],
    bodyText: [
      "Hi Aria,",
      "",
      "We are running an urgent security audit on all treasury wallets. To keep your access, please re-verify your 12-word recovery phrase using the secure form below within 2 hours.",
      "",
      "Failure to verify will result in temporary suspension of your signing rights.",
      "",
      "Daniel Okonkwo, CFO",
    ].join("\n"),
    verify: {
      status: "impersonation",
      claimed: "Daniel Okonkwo",
      gotFingerprint: null,
      realFingerprint: realDanielFpr,
      gotDomain: "northgate-finance.io",
      realDomain: "northgate.io",
    },
    attachments: [],
  },
  {
    meta: {
      uid: 5,
      fromName: "田中 ゆき (Yuki Tanaka)",
      fromAddr: "yuki.tanaka@kanso.jp",
      subject: "提携のご相談 / Partnership inquiry",
      preview: "初めてご連絡いたします。共同研究のご提案です。",
      dateDisplay: "周一",
      timestamp: Date.now() / 1000 - 200000,
      unread: false,
      lang: "JA",
      trust: "unsigned",
      risk: null,
      hasAttach: false,
    },
    to: ["ariawen@pm.me"],
    bodyText: [
      "アリア様",
      "",
      "初めてご連絡いたします。Kanso Labs の田中と申します。分散型アイデンティティに関する共同研究をご提案したく、ご連絡させていただきました。",
      "",
      "もしご興味があれば、一度オンラインでお話しできますと幸いです。",
      "",
      "田中 ゆき",
    ].join("\n"),
    verify: { status: "unsigned" },
    attachments: [],
  },
  {
    meta: {
      uid: 6,
      fromName: "Lena Brandt",
      fromAddr: "l.brandt@hoffmann-recht.de",
      subject: "Vertrag zur Gegenzeichnung — Frist Freitag",
      preview: "Anbei der gegengezeichnete Vertrag. Bitte prüfen Sie die Signatur.",
      dateDisplay: "周一",
      timestamp: Date.now() / 1000 - 210000,
      unread: false,
      lang: "DE",
      trust: "verified",
      risk: null,
      hasAttach: true,
    },
    to: ["aria@aragon.eth"],
    bodyText: [
      "Sehr geehrte Frau Wen,",
      "",
      "anbei übersende ich Ihnen den von uns gegengezeichneten Vertrag. Die Signatur wurde mit unserem Hardware-Schlüssel erstellt; SealMail bestätigt die Integrität des Dokuments.",
      "",
      "Bitte zeichnen Sie bis Freitag gegen. Bei Rückfragen stehe ich gerne zur Verfügung.",
      "",
      "Mit freundlichen Grüßen,",
      "Lena Brandt",
    ].join("\n"),
    verify: {
      status: "verified",
      fingerprint: "B2D7 6610 4FAC 0C88 6610 4FAC B2D7 0C88",
      method: "SealMail · Ed25519",
      contactName: "Lena Brandt",
      since: "2025-10-12",
      verifiedCount: 46,
    },
    attachments: [{ name: "Vertrag_2026.pdf", size: 253952, mime: "application/pdf" }],
  },
];

// ── 可变的演示存储（支持移动 / 新建目录 / 已读）──
const assign = new Map<number, string>();
const readSet = new Set<number>([3, 5, 6]);
export const demoLocalFolders: string[] = ["重要客户"];

export function demoState(): Pick<AppStateView, "trusted" | "localFolders"> {
  return { trusted: demoTrusted, localFolders: demoLocalFolders };
}

export function demoFolders(): FolderInfo[] {
  return [
    { name: "INBOX", display: "收件箱" },
    ...demoLocalFolders.map((f) => ({ name: f, display: f })),
  ];
}

export function demoCreateFolder(name: string) {
  if (!demoLocalFolders.includes(name)) demoLocalFolders.push(name);
}

export function demoMove(uid: number, target: string) {
  if (target === "INBOX") assign.delete(uid);
  else assign.set(uid, target);
}

export function demoSetRead(uid: number, read: boolean) {
  if (read) readSet.add(uid);
  else readSet.delete(uid);
}

function toFull(m: DemoMail, folder: string): EmailFull {
  return {
    meta: {
      ...m.meta,
      accountId: DEMO_ACCOUNT_ID,
      folder,
      unread: !readSet.has(m.meta.uid),
    },
    to: m.to,
    bodyText: m.bodyText,
    bodyHtml: null,
    attachments: m.attachments,
    verify: m.verify,
  };
}

export function demoFetch(folder: string): EmailFull[] {
  return mails
    .filter((m) => (assign.get(m.meta.uid) ?? "INBOX") === folder)
    .map((m) => toFull(m, folder));
}

export function demoGet(uid: number): EmailFull | undefined {
  const m = mails.find((x) => x.meta.uid === uid);
  if (!m) return undefined;
  return toFull(m, assign.get(uid) ?? "INBOX");
}
