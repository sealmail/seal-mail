export type RequestToken = number;

export class LatestRequest {
  private current = 0;

  begin(): RequestToken {
    this.current += 1;
    return this.current;
  }

  invalidate(): void {
    this.current += 1;
  }

  isCurrent(token: RequestToken): boolean {
    return token === this.current;
  }
}
