import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { Sidebar, UNIFIED_FOLDER } from "./Sidebar";

test("shows the aggregated unread count for all inboxes", () => {
  // 统一 = 各账户 INBOX 全量 COUNT 之和；收件箱 = 当前账户全量 COUNT（不是列表窗口）
  const html = renderToStaticMarkup(
    <Sidebar
      accounts={[]}
      currentAccountId=""
      folders={[
        { name: UNIFIED_FOLDER, display: "统一收件箱" },
        { name: "INBOX", display: "收件箱" },
      ]}
      currentFolder="INBOX"
      riskCount={0}
      unifiedUnread={551}
      inboxUnread={318}
      draftCount={0}
      view="mail"
      onSelectAccount={() => {}}
      onSelectFolder={() => {}}
      onOpenKeys={() => {}}
      onAddAccount={() => {}}
      onRemoveAccount={() => {}}
      onNewFolder={() => {}}
      onDeleteFolder={() => {}}
      onOpenFilters={() => {}}
    />
  );

  expect(html).toContain('<span class="label">统一收件箱</span><span class="count">551</span>');
  expect(html).toContain('<span class="label">收件箱</span><span class="count">318</span>');
  expect(html).not.toContain("key-status");
});

test("keeps organization and account controls outside the folder scroller", () => {
  const html = renderToStaticMarkup(
    <Sidebar
      accounts={[]}
      currentAccountId=""
      folders={[]}
      currentFolder="INBOX"
      riskCount={0}
      unifiedUnread={0}
      inboxUnread={0}
      draftCount={0}
      view="mail"
      onSelectAccount={() => {}}
      onSelectFolder={() => {}}
      onOpenKeys={() => {}}
      onAddAccount={() => {}}
      onRemoveAccount={() => {}}
      onNewFolder={() => {}}
      onDeleteFolder={() => {}}
      onOpenFilters={() => {}}
    />
  );

  const folderScroller = html.match(/<div class="sidebar-scroll">([\s\S]*?)<\/div><div class="sidebar-pinned">/);
  expect(folderScroller).not.toBeNull();
  expect(folderScroller?.[1]).toContain("+ 新建目录");
  expect(folderScroller?.[1]).not.toContain("整理");
  expect(folderScroller?.[1]).not.toContain("已连接账户");
});
