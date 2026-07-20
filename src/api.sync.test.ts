import { describe, expect, test } from "bun:test";
import { createSyncCoalescer, type SyncResult } from "./api";

/**
 * 直接测 api.ts 导出的真实合并实现（syncMessages/syncOlderMessages 内部就是它），
 * 通过可注入的 run 替代 Tauri invoke。若把 api.ts 里的 inflight 合并逻辑删掉，
 * 这些测试必须失败。
 */

function tick() {
  return new Promise<void>((r) => setTimeout(r, 0));
}

describe("createSyncCoalescer", () => {
  test("concurrent sync for same account/folder coalesces to one invoke", async () => {
    const calls: string[] = [];
    let resolve!: (r: SyncResult) => void;
    const c = createSyncCoalescer((kind, accountId, folder) => {
      calls.push(`${kind}:${accountId}:${folder}`);
      return new Promise<SyncResult>((r) => {
        resolve = r;
      });
    });
    const a = c.sync("acc", "INBOX");
    const b = c.sync("acc", "INBOX");
    expect(calls).toEqual(["sync:acc:INBOX"]);
    resolve({ added: 7, total: 10 });
    expect((await a).added).toBe(7);
    expect((await b).added).toBe(7);
    // 上一轮完成后，新的 sync 必须真正发起新调用（inflight 表已清理）
    const again = c.sync("acc", "INBOX");
    expect(calls).toEqual(["sync:acc:INBOX", "sync:acc:INBOX"]);
    resolve({ added: 0, total: 10 });
    expect((await again).total).toBe(10);
  });

  test("different folders do not coalesce", async () => {
    const calls: string[] = [];
    const c = createSyncCoalescer(async (kind, accountId, folder) => {
      calls.push(`${kind}:${accountId}:${folder}`);
      return { added: folder === "INBOX" ? 1 : 2, total: 3 };
    });
    const p1 = c.sync("acc", "INBOX");
    const p2 = c.sync("acc", "Sent");
    expect(calls).toEqual(["sync:acc:INBOX", "sync:acc:Sent"]);
    expect((await p1).added).toBe(1);
    expect((await p2).added).toBe(2);
  });

  test("failed sync propagates the error and clears the inflight slot", async () => {
    let n = 0;
    const c = createSyncCoalescer((kind) => {
      n += 1;
      if (n === 1) return Promise.reject(new Error(`${kind} boom`));
      return Promise.resolve({ added: 4, total: 4 });
    });
    await expect(c.sync("acc", "INBOX")).rejects.toThrow("sync boom");
    // 失败后重试必须发起新调用而不是复用已失败的 Promise
    expect((await c.sync("acc", "INBOX")).added).toBe(4);
    expect(n).toBe(2);
  });

  test("sync-older waits for the in-flight sync of the same folder", async () => {
    const calls: string[] = [];
    const resolvers = new Map<string, (r: SyncResult) => void>();
    const c = createSyncCoalescer((kind, accountId, folder) => {
      const key = `${kind}:${accountId}:${folder}`;
      calls.push(key);
      return new Promise<SyncResult>((r) => {
        resolvers.set(key, r);
      });
    });
    const syncP = c.sync("acc", "INBOX");
    const olderP = c.syncOlder("acc", "INBOX");
    await tick();
    // 增量 sync 在飞时，sync-older 不得同时开跑（后端目录锁会硬撞）
    expect(calls).toEqual(["sync:acc:INBOX"]);
    resolvers.get("sync:acc:INBOX")!({ added: 1, total: 1 });
    await syncP;
    await tick();
    expect(calls).toEqual(["sync:acc:INBOX", "sync-older:acc:INBOX"]);
    resolvers.get("sync-older:acc:INBOX")!({ added: 0, total: 1 });
    expect((await olderP).added).toBe(0);
  });

  test("sync waits for the in-flight sync-older of the same folder", async () => {
    const calls: string[] = [];
    const resolvers = new Map<string, (r: SyncResult) => void>();
    const c = createSyncCoalescer((kind, accountId, folder) => {
      const key = `${kind}:${accountId}:${folder}`;
      calls.push(key);
      return new Promise<SyncResult>((r) => {
        resolvers.set(key, r);
      });
    });
    const olderP = c.syncOlder("acc", "INBOX");
    const syncP = c.sync("acc", "INBOX");
    await tick();
    expect(calls).toEqual(["sync-older:acc:INBOX"]);
    resolvers.get("sync-older:acc:INBOX")!({ added: 2, total: 2 });
    await olderP;
    await tick();
    expect(calls).toEqual(["sync-older:acc:INBOX", "sync:acc:INBOX"]);
    resolvers.get("sync:acc:INBOX")!({ added: 3, total: 5 });
    expect((await syncP).added).toBe(3);
  });

  test("concurrent sync-older for same folder coalesces", async () => {
    const calls: string[] = [];
    let resolve!: (r: SyncResult) => void;
    const c = createSyncCoalescer((kind, accountId, folder) => {
      calls.push(`${kind}:${accountId}:${folder}`);
      return new Promise<SyncResult>((r) => {
        resolve = r;
      });
    });
    const a = c.syncOlder("acc", "INBOX");
    const b = c.syncOlder("acc", "INBOX");
    expect(calls).toEqual(["sync-older:acc:INBOX"]);
    resolve({ added: 5, total: 9 });
    expect((await a).added).toBe(5);
    expect((await b).added).toBe(5);
  });
});
