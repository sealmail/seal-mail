import { describe, expect, test } from "bun:test";
import { EMAIL_DOCUMENT_BASE_CSS } from "./HtmlBody";

describe("HTML email document styling", () => {
  test("uses a pure white background for transparent email regions", () => {
    expect(EMAIL_DOCUMENT_BASE_CSS).toContain("html { background: #fff;");
    expect(EMAIL_DOCUMENT_BASE_CSS).toContain("body { background: #fff;");
  });
});
