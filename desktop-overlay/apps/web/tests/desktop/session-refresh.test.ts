import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type SessionRefresh = typeof import("../../../../packages/services/src/session-refresh");
let session: SessionRefresh;
const api = "https://api.example.test";

beforeEach(async () => {
  vi.resetModules();
  session = await import("../../../../packages/services/src/session-refresh");
});
afterEach(() => {
  delete (window as any).__PIDASH_NATIVE_HTTP__;
  delete (window as any).__PIDASH_NATIVE_SESSION__;
  delete (window as any).__TAURI__;
});

describe("page session refresh", () => {
  it("has nothing to refresh with in an edition that registers no refresher", () => {
    expect(session.refreshSession(api)).toBeUndefined();
  });

  it("shares one attempt between everything that asks while it is in flight", async () => {
    let finish = () => {};
    const refresher = vi.fn(() => new Promise<void>((resolve) => (finish = resolve)));
    session.registerSessionRefresher(refresher);
    const first = session.refreshSession(api);
    const second = session.refreshSession(api);
    expect(second).toBe(first);
    expect(refresher).toHaveBeenCalledOnce();
    expect(refresher).toHaveBeenCalledWith(api);
    finish();
    await first;
    // A later 401 starts a new attempt.
    void session.refreshSession(api);
    expect(refresher).toHaveBeenCalledTimes(2);
  });

  it("shares a failed attempt, then tries again", async () => {
    const refresher = vi.fn().mockRejectedValueOnce(new Error("offline")).mockResolvedValue(undefined);
    session.registerSessionRefresher(refresher);
    const first = session.refreshSession(api);
    const second = session.refreshSession(api);
    await expect(first).rejects.toThrow("offline");
    await expect(second).rejects.toThrow("offline");
    await expect(session.refreshSession(api)).resolves.toBeUndefined();
    expect(refresher).toHaveBeenCalledTimes(2);
  });

  it("stands down when the desktop transport refreshes the session itself", () => {
    const refresher = vi.fn(async () => {});
    session.registerSessionRefresher(refresher);
    Object.assign(window, { __PIDASH_NATIVE_HTTP__: api, __TAURI__: { core: {} } });
    (window as any).__PIDASH_NATIVE_SESSION__ = { refresh: true, strict: true };
    expect(session.refreshSession(api)).toBeUndefined();
    expect(refresher).not.toHaveBeenCalled();
    // A binary whose transport does not refresh leaves the page in charge.
    (window as any).__PIDASH_NATIVE_SESSION__ = { refresh: false, strict: true };
    expect(session.refreshSession(api)).toBeInstanceOf(Promise);
    expect(refresher).toHaveBeenCalledOnce();
  });
});
