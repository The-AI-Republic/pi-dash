// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Provider sign-in mechanics (parity/specs/cloud.md §7, CLOUD-033). The
// tab must open inside the click; an await before window.open loses the
// user's activation and browsers block it. So a blank tab is reserved
// first, detached from its opener so the provider page cannot navigate the
// product, and only then pointed at the authorize address. Polling lives
// here as a plain driver so the page can own it and a unit test can run it
// against a fake clock.

export interface SignInStart {
  authorize_url: string;
  state: string;
}

export interface PopupLike {
  closed: boolean;
  opener: unknown;
  location: { replace(url: string): void };
  close(): void;
}

export type PopupOpener = () => PopupLike | null;

export type SignInStartFailure = { kind: "blocked" } | { kind: "closed" } | { kind: "rejected"; error: unknown };

/** Either the started flow, or why it never started. */
export type SignInStartResult = { result: SignInStart; failure: null } | { result: null; failure: SignInStartFailure };

/**
 * Reserve the tab during the click, start the flow, and point the tab at
 * the authorize address. A blocked tab, a tab closed before the server
 * answered, and a rejected start are distinct outcomes with distinct
 * reports. A rejected start also closes the reserved tab.
 */
export async function beginProviderSignIn(
  open: PopupOpener,
  start: () => Promise<SignInStart>
): Promise<SignInStartResult> {
  const popup = open();
  if (popup === null) return { result: null, failure: { kind: "blocked" } };
  popup.opener = null;
  try {
    const started = await start();
    if (popup.closed) {
      return { result: null, failure: { kind: "closed" } };
    }
    const result: SignInStart = { authorize_url: started.authorize_url, state: started.state };
    popup.location.replace(result.authorize_url);
    return { result, failure: null };
  } catch (error) {
    popup.close();
    return { result: null, failure: { kind: "rejected", error } };
  }
}

export type PollOutcome = { kind: "pending" } | { kind: "confirmed" } | { kind: "failed"; detail: string | undefined };

export function pollOutcome(status: {
  status: "pending" | "ok" | "error" | "expired";
  detail?: string | undefined;
}): PollOutcome {
  switch (status.status) {
    case "pending":
      return { kind: "pending" };
    case "ok":
      return { kind: "confirmed" };
    case "error":
    case "expired":
      return { kind: "failed", detail: status.detail };
  }
}

export const SIGN_IN_POLL_INTERVAL_MS = 2_000;
export const SIGN_IN_POLL_TIMEOUT_MS = 5 * 60_000;

export interface PollClock {
  now(): number;
  setInterval(callback: () => void, delay: number): unknown;
  clearInterval(handle: unknown): void;
}

export const browserClock: PollClock = {
  now: () => Date.now(),
  setInterval: (callback, delay) => globalThis.setInterval(callback, delay),
  clearInterval: (handle) => globalThis.clearInterval(handle as ReturnType<typeof globalThis.setInterval>),
};

export type SignInFinish = { kind: "confirmed" } | { kind: "failed"; detail: string | undefined } | { kind: "timeout" };

/**
 * Poll the login status until it settles or the deadline passes. A
 * transient read failure is ignored — the provider's callback may still
 * complete — while a timeout always ends the flow. Returns a stop handle so
 * the caller can cancel from a button or an unmount.
 */
export function pollProviderLogin(options: {
  read: () => Promise<{ status: "pending" | "ok" | "error" | "expired"; detail?: string | undefined }>;
  onFinish: (finish: SignInFinish) => void;
  clock?: PollClock;
  intervalMs?: number;
  timeoutMs?: number;
}): { stop(): void } {
  const clock = options.clock ?? browserClock;
  const intervalMs = options.intervalMs ?? SIGN_IN_POLL_INTERVAL_MS;
  const timeoutMs = options.timeoutMs ?? SIGN_IN_POLL_TIMEOUT_MS;
  const startedAt = clock.now();
  let handle: unknown = null;
  let stopped = false;

  const stop = (): void => {
    if (stopped) return;
    stopped = true;
    if (handle !== null) clock.clearInterval(handle);
    handle = null;
  };

  const tick = async (): Promise<void> => {
    if (stopped) return;
    if (clock.now() - startedAt > timeoutMs) {
      stop();
      options.onFinish({ kind: "timeout" });
      return;
    }
    let answer: Awaited<ReturnType<typeof options.read>>;
    try {
      answer = await options.read();
    } catch {
      return;
    }
    if (stopped) return;
    const outcome = pollOutcome(answer);
    if (outcome.kind === "pending") return;
    stop();
    options.onFinish(outcome.kind === "confirmed" ? { kind: "confirmed" } : { kind: "failed", detail: outcome.detail });
  };

  handle = clock.setInterval(() => {
    void tick();
  }, intervalMs);
  return { stop };
}
