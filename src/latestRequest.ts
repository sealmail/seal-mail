export type RequestToken = number;

export class LatestRequest {
  private current = 0;

  begin(): RequestToken {
    this.current += 1;
    return this.current;
  }

  /** 当前有效 token（不推进序号）。用于软刷新：不打断正在进行的拉取。 */
  token(): RequestToken {
    return this.current;
  }

  invalidate(): void {
    this.current += 1;
  }

  isCurrent(token: RequestToken): boolean {
    return token === this.current;
  }
}
