import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { MailList } from "./MailList";

test("category tabs show category totals without duplicating unread badges", () => {
  const html = renderToStaticMarkup(
    <MailList
      messages={[]}
      selectedKey={null}
      loading={false}
      syncing={false}
      error={null}
      filterMode="all"
      categoryMode="all"
      categoryCounts={{ all: 186, personal: 0, business: 186, ads: 0 }}
      unreadCount={24}
      loadedCount={186}
      totalCount={186}
      hasMore={false}
      loadingMore={false}
      onFilterMode={() => {}}
      onCategoryMode={() => {}}
      onMarkAllRead={() => {}}
      onToggleFlag={() => {}}
      onLoadMore={() => {}}
      onSelect={() => {}}
      onOpenWindow={() => {}}
      onRefresh={() => {}}
    />
  );

  expect(html).toContain("未读 24");
  expect(html).toContain("商务<span>186</span>");
  expect(html).toContain("全部<span>186</span>");
  expect(html).not.toContain("category-unread");
});

test("list status distinguishes local cache from server backfill", () => {
  const html = renderToStaticMarkup(
    <MailList
      messages={[]}
      selectedKey={null}
      loading={false}
      syncing={false}
      error={null}
      filterMode="all"
      categoryMode="all"
      categoryCounts={{ all: 0, personal: 0, business: 0, ads: 0 }}
      unreadCount={0}
      loadedCount={0}
      totalCount={0}
      hasMore={true}
      loadingMore={false}
      onFilterMode={() => {}}
      onCategoryMode={() => {}}
      onMarkAllRead={() => {}}
      onToggleFlag={() => {}}
      onLoadMore={() => {}}
      onSelect={() => {}}
      onOpenWindow={() => {}}
      onRefresh={() => {}}
    />
  );
  expect(html).toContain("可从服务器补全更早邮件");
});
