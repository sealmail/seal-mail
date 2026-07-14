import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { SendCountdownDialog } from "./SendCountdownDialog";

describe("SendCountdownDialog", () => {
  test("shows a prominent countdown with undo as the only action", () => {
    const html = renderToStaticMarkup(
      <SendCountdownDialog
        seconds={10}
        title="邮件将在稍后发送"
        description="你可以在倒计时结束前撤销发送"
        statusLabel="等待发送"
        secondsLabel="秒"
        undoLabel="撤销发送"
        onUndo={() => {}}
      />
    );

    expect(html).toContain('role="alertdialog"');
    expect(html).toContain('aria-live="assertive"');
    expect(html).toContain(">10<");
    expect(html).toContain("撤销发送");
    expect(html).not.toContain("立即发送");
    expect((html.match(/<button/g) ?? []).length).toBe(1);
  });
});
