import { useI18n } from "../i18n";
import { shortFpr } from "../trust";
import type { IdentityInfo } from "../types";
import { AppIcon } from "./AppIcon";

interface Props {
  identity: IdentityInfo | null;
  onAddAccount: () => void;
  onOpenKeys: () => void;
}

/** 首次使用引导：没有任何账户时显示，替代主三栏区域 */
export function Onboarding(p: Props) {
  const t = useI18n();
  const idLabel =
    p.identity?.mode === "ledger"
      ? `Ledger ${p.identity.ledgerAddress ? shortAddr(p.identity.ledgerAddress) : ""}`
      : p.identity
        ? t("本地密钥 {fpr}", { fpr: shortFpr(p.identity.fingerprint) })
        : t("正在生成…");

  return (
    <div className="onboard">
      <div className="onboard-card">
        <AppIcon className="onboard-icon" />
        <h1>{t("欢迎使用 SealMail 信印")}</h1>
        <p className="lead">
          {t("一个通用邮件客户端，外加一层「证明邮件可信」的能力。开始前，请先连接你的邮箱账户。")}
        </p>

        <div className="onboard-steps">
          <div className="onboard-step">
            <div className="num">1</div>
            <div className="body">
              <div className="title">{t("添加邮箱账户")}</div>
              <div className="desc">
                {t("支持 IMAP / POP3 + SMTP。内置 Exchange (Office 365 / 自建)、Gmail、iCloud、QQ、163 预设，也可以手动填服务器。密码只保存在本机。")}
              </div>
              <button className="btn-primary" style={{ height: 38, padding: "0 22px", marginTop: 10 }} onClick={p.onAddAccount}>
                {t("+ 添加邮箱账户")}
              </button>
            </div>
          </div>

          <div className="onboard-step">
            <div className="num">2</div>
            <div className="body">
              <div className="title">{t("签名身份（已就绪，可选配置）")}</div>
              <div className="desc">
                {t("已为你生成本机签名密钥：")}<span className="mono">{idLabel}</span>{t("。发邮件时可选择签名——对方若也用 SealMail 会看到可验证的封印；普通邮箱收件人只会看到一行低调的签名说明。也可以改用 Ledger 硬件密钥签名。")}
              </div>
              <button className="btn-ghost" style={{ height: 34, marginTop: 10 }} onClick={p.onOpenKeys}>
                {t("身份与密钥设置 →")}
              </button>
            </div>
          </div>
        </div>

        <div className="onboard-foot">
          {t("收到签名邮件时，SealMail 在本地验证发件人身份与内容完整性——不依赖头像、邮件头或语言判断真伪。")}
        </div>
      </div>
    </div>
  );
}

function shortAddr(addr: string) {
  return addr.length > 12 ? `${addr.slice(0, 6)}…${addr.slice(-4)}` : addr;
}
