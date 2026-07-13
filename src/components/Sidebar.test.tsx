import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { Sidebar, UNIFIED_FOLDER } from "./Sidebar";

test("shows the aggregated unread count for all inboxes", () => {
  const html = renderToStaticMarkup(
    <Sidebar
      identity={null}
      accounts={[]}
      currentAccountId=""
      folders={[
        { name: UNIFIED_FOLDER, display: "统一收件箱" },
        { name: "INBOX", display: "收件箱" },
      ]}
      currentFolder="INBOX"
      riskCount={0}
      unifiedUnread={34}
      inboxUnread={29}
      draftCount={0}
      view="mail"
      ledgerMode={false}
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

  expect(html).toContain('<span class="label">统一收件箱</span><span class="count">34</span>');
  expect(html).toContain('<span class="label">收件箱</span><span class="count">29</span>');
});

test("keeps organization and account controls outside the folder scroller", () => {
  const html = renderToStaticMarkup(
    <Sidebar
      identity={null}
      accounts={[]}
      currentAccountId=""
      folders={[]}
      currentFolder="INBOX"
      riskCount={0}
      unifiedUnread={0}
      inboxUnread={0}
      draftCount={0}
      view="mail"
      ledgerMode={false}
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
