import { describe, expect, test } from "bun:test";
import styles from "./styles.css" with { type: "text" };

describe("window surface colors", () => {
  test("uses pure white instead of warm white for every base surface", () => {
    expect(styles).toContain("--bg: #fff;");
    expect(styles).toContain("--bg-warm: #fff;");
    expect(styles).toContain("--bg-side: #fff;");

    for (const warmWhite of ["#fbfbf8", "#f6f4ef", "#f4f6f2", "#f8f6ef", "#fffdfa", "rgba(251, 251, 248"]) {
      expect(styles).not.toContain(warmWhite);
    }
  });
});

describe("compose experience", () => {
  test("gives the composer and message body substantial working space", () => {
    expect(styles).toContain(".compose-modal {");
    expect(styles).toContain("width: min(920px, calc(100vw - 48px));");
    expect(styles).toContain("height: min(820px, calc(100vh - 48px));");
    expect(styles).toContain(".compose-body-input {");
    expect(styles).toContain("flex: 1;");
    expect(styles).toContain("min-height: 280px;");
  });

  test("presents undo send as a separate prominent dialog", () => {
    expect(styles).toContain(".send-countdown-overlay {");
    expect(styles).toContain(".send-countdown-dialog {");
    expect(styles).toContain("font-variant-numeric: tabular-nums;");
  });
});
