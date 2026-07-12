import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { Sidebar } from "./Sidebar";

test("keeps organization and account controls outside the folder scroller", () => {
  const html = renderToStaticMarkup(
    <Sidebar
      identity={null}
      accounts={[]}
      currentAccountId=""
      folders={[]}
      currentFolder="INBOX"
      riskCount={0}
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
