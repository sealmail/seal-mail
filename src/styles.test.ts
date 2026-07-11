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
