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
});
