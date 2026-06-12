import type { CSSProperties } from "react";
import type { TrustTag } from "../types";

/** 火漆封印：
 *  verified=完整封印(青玉) · signedUnknown=灰绿封印(签名有效·未列入可信)
 *  unsigned=未验证盾牌 · tampered=裂开的封印 · impersonation=伪造封印(划痕+警示徽章) */
export function Seal({ trust, size }: { trust: TrustTag; size: number }) {
  const fs = Math.round(size * 0.42);
  const ring = Math.max(1.4, size * 0.045);
  const wrap: CSSProperties = {
    position: "relative",
    width: size,
    height: size,
    flexShrink: 0,
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
  };
  const stamp = (col: string): CSSProperties => ({
    position: "absolute",
    inset: 0,
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    fontFamily: '"IBM Plex Serif", serif',
    fontWeight: 600,
    fontSize: fs,
    color: col,
    textShadow: "0 1px 1px rgba(0,0,0,.3), 0 -1px 1px rgba(255,255,255,.2)",
  });

  if (trust === "verified") {
    return (
      <div style={wrap}>
        <div
          style={{
            width: size,
            height: size,
            borderRadius: "50%",
            background: "radial-gradient(circle at 36% 30%, #4CA67E 0%, #2C7B58 52%, #1B5840 100%)",
            boxShadow: `0 0 0 ${ring}px #5F695F, inset 0 ${size * 0.04}px ${size * 0.08}px rgba(255,255,255,.45), inset 0 -${size * 0.05}px ${size * 0.1}px rgba(0,0,0,.32)`,
          }}
        />
        <div style={stamp("rgba(255,255,255,.92)")}>印</div>
      </div>
    );
  }

  if (trust === "signedUnknown") {
    return (
      <div style={wrap}>
        <div
          style={{
            width: size,
            height: size,
            borderRadius: "50%",
            background: "radial-gradient(circle at 36% 30%, #8A978B 0%, #657164 54%, #3F4B42 100%)",
            boxShadow: `inset 0 ${size * 0.04}px ${size * 0.08}px rgba(255,255,255,.38), inset 0 -${size * 0.05}px ${size * 0.1}px rgba(0,0,0,.3)`,
          }}
        />
        <div style={stamp("rgba(255,255,255,.92)")}>印</div>
      </div>
    );
  }

  if (trust === "unsigned") {
    return (
      <div style={wrap}>
        <div
          style={{
            width: size,
            height: size,
            border: `${Math.max(1.5, size * 0.045)}px solid #92978f`,
            borderRadius: "34%",
            background: "linear-gradient(180deg, rgba(218,220,214,.98), rgba(239,240,236,.9))",
          }}
        />
        <div style={{ ...stamp("#636960"), fontFamily: "var(--sans)", textShadow: "none", fontWeight: 800, fontSize: Math.round(size * 0.42) }}>
          !
        </div>
      </div>
    );
  }

  // 朱红底（tampered / impersonation 共用）
  const disc = (
    <div
      style={{
        width: size,
        height: size,
        borderRadius: "50%",
        background: "radial-gradient(circle at 36% 30%, #C24B3A 0%, #9A2C1D 54%, #6E2016 100%)",
        boxShadow: `inset 0 ${size * 0.04}px ${size * 0.08}px rgba(255,255,255,.3), inset 0 -${size * 0.05}px ${size * 0.1}px rgba(0,0,0,.4)`,
      }}
    />
  );

  if (trust === "tampered") {
    return (
      <div style={wrap}>
        {disc}
        <div style={stamp("rgba(255,255,255,.55)")}>印</div>
        <div
          style={{
            position: "absolute",
            top: -size * 0.06,
            height: size * 1.12,
            width: Math.max(2, size * 0.07),
            left: "52%",
            transform: "translateX(-50%) rotate(13deg)",
            background: "linear-gradient(90deg, rgba(255,255,255,.18), rgba(0,0,0,.5) 50%, rgba(255,255,255,.12))",
            clipPath:
              "polygon(40% 0,62% 18%,44% 34%,60% 52%,42% 70%,58% 86%,46% 100%,36% 100%,50% 84%,34% 66%,52% 50%,36% 32%,54% 16%,40% 0)",
          }}
        />
      </div>
    );
  }

  // impersonation：伪造封印 — 白色划痕 + 角标警示
  return (
    <div style={wrap}>
      {disc}
      <div style={stamp("rgba(255,255,255,.5)")}>印</div>
      <div
        style={{
          position: "absolute",
          width: size * 1.18,
          height: Math.max(2, size * 0.08),
          background: "rgba(255,255,255,.85)",
          boxShadow: "0 0 2px rgba(0,0,0,.4)",
          transform: "rotate(-45deg)",
        }}
      />
      <div
        style={{
          position: "absolute",
          top: -size * 0.12,
          right: -size * 0.12,
          width: size * 0.52,
          height: size * 0.52,
          minWidth: 14,
          minHeight: 14,
          borderRadius: "50%",
          background: "#F7E8E5",
          border: "1.5px solid #B23A2B",
          color: "#B23A2B",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          fontSize: Math.max(9, size * 0.28),
          fontWeight: 800,
        }}
      >
        !
      </div>
    </div>
  );
}
