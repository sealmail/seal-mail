import { describe, expect, test } from "bun:test";

/**
 * 同步合并逻辑与 api.ts 内 inflight 表一致：同一 key 应复用同一 Promise。
 * 这里用可注入的模拟实现验证合并语义（不依赖 Tauri invoke）。
 */
function makeSyncCoalescer() {
  const inflight = new Map<string, Promise<number>>();
  let calls = 0;
  async function sync(accountId: string, folder: string, work: () => Promise<number>) {
    const key = `${accountId}\0${folder}`;
    const existing = inflight.get(key);
    if (existing) return existing;
    calls += 1;
    const p = work().finally(() => {
      if (inflight.get(key) === p) inflight.delete(key);
    });
    inflight.set(key, p);
    return p;
  }
  return {
    sync,
    get calls() {
      return calls;
    },
  };
}

describe("syncMessages coalescing", () => {
  test("concurrent sync for same account/folder shares one in-flight call", async () => {
    const c = makeSyncCoalescer();
    let resolve!: (n: number) => void;
    const work = () =>
      new Promise<number>((r) => {
        resolve = r;
      });
    const a = c.sync("acc", "INBOX", work);
    const b = c.sync("acc", "INBOX", work);
    expect(c.calls).toBe(1);
    resolve(7);
    expect(await a).toBe(7);
    expect(await b).toBe(7);
  });

  test("different folders do not coalesce", async () => {
    const c = makeSyncCoalescer();
    const p1 = c.sync("acc", "INBOX", async () => 1);
    const p2 = c.sync("acc", "Sent", async () => 2);
    expect(c.calls).toBe(2);
    expect(await p1).toBe(1);
    expect(await p2).toBe(2);
  });
});
