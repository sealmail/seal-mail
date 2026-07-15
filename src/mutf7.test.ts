import { expect, test } from "bun:test";
import { decodeMutf7, folderTitle } from "./mutf7";

test("decodeMutf7 decodes RFC 3501 examples and Chinese folder names", () => {
  expect(decodeMutf7("&U,BTFw-")).toBe("台北");
  expect(decodeMutf7("a&-b")).toBe("a&b");
  expect(decodeMutf7("INBOX/Sent")).toBe("INBOX/Sent");
  // QQ 等服务商「垃圾邮件」目录的 modified UTF-7 名
  expect(decodeMutf7("&V4NXPpCuTvY-")).toBe("垃圾邮件");
});

test("folderTitle prefers display and falls back to mutf7 decode", () => {
  expect(folderTitle("&V4NXPpCuTvY-", "垃圾邮件")).toBe("垃圾邮件");
  expect(folderTitle("&V4NXPpCuTvY-")).toBe("垃圾邮件");
  expect(folderTitle("INBOX", "收件箱")).toBe("收件箱");
});
