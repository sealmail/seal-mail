import { describe, expect, test } from "bun:test";
import { LatestRequest } from "./latestRequest";

describe("LatestRequest", () => {
  test("does not let the previous folder commit after the user switches folders", () => {
    const requests = new LatestRequest();
    const inboxRequest = requests.begin();
    let visibleFolder = "robot";

    requests.invalidate();
    if (requests.isCurrent(inboxRequest)) visibleFolder = "INBOX";

    expect(visibleFolder).toBe("robot");
  });

  test("token() returns the current sequence without advancing it", () => {
    const requests = new LatestRequest();
    expect(requests.token()).toBe(0);
    const first = requests.begin();
    expect(requests.token()).toBe(first);
    expect(requests.token()).toBe(first);
    expect(requests.isCurrent(first)).toBe(true);
  });
});
