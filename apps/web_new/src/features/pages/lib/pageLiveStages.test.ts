// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Collaboration reconnect machine (parity/specs/pages.md §8.4, PAGE-067):
// the walk from the first dial through connecting, awaiting-sync and
// synced, with counted reconnect attempts into terminal disconnected,
// plus the badge mapping and the backoff.

import { describe, expect, it } from "vitest";

import {
  nextPageLiveStage,
  PAGE_LIVE_CONNECT_TIMEOUT_MS,
  PAGE_LIVE_MAX_ATTEMPTS,
  PAGE_LIVE_RETRY_BASE_MS,
  PAGE_LIVE_RETRY_CAP_MS,
  pageLiveRetryDelayMs,
  pageLiveSyncReport,
  type PageLiveStage,
} from "./pageLiveStages.js";

const INITIAL: PageLiveStage = { kind: "initial" };
const CONNECTING: PageLiveStage = { kind: "connecting" };
const AWAITING: PageLiveStage = { kind: "awaiting-sync" };
const SYNCED: PageLiveStage = { kind: "synced" };
const RECONNECTING: PageLiveStage = { kind: "reconnecting", attempt: 2 };
const DISCONNECTED: PageLiveStage = { kind: "disconnected", reason: "exhausted" };
const LIVE: PageLiveStage[] = [CONNECTING, AWAITING, SYNCED, RECONNECTING];

describe("nextPageLiveStage", () => {
  it("starts the dial from rest or terminal", () => {
    expect(nextPageLiveStage(INITIAL, { type: "start" })).toEqual({ kind: "connecting" });
    expect(nextPageLiveStage(DISCONNECTED, { type: "start" })).toEqual({ kind: "connecting" });
  });

  it("ignores a start mid-walk", () => {
    for (const stage of LIVE) expect(nextPageLiveStage(stage, { type: "start" })).toBe(stage);
  });

  it("waits for sync once the socket opens", () => {
    expect(nextPageLiveStage(CONNECTING, { type: "opened" })).toEqual({ kind: "awaiting-sync" });
    expect(nextPageLiveStage(RECONNECTING, { type: "opened" })).toEqual({ kind: "awaiting-sync" });
  });

  it("ignores a stray open", () => {
    for (const stage of [INITIAL, AWAITING, SYNCED, DISCONNECTED]) {
      expect(nextPageLiveStage(stage, { type: "opened" })).toBe(stage);
    }
  });

  it("lands synced from anywhere mid-walk", () => {
    for (const stage of LIVE) expect(nextPageLiveStage(stage, { type: "synced" })).toEqual({ kind: "synced" });
  });

  it("ignores a stray sync", () => {
    expect(nextPageLiveStage(INITIAL, { type: "synced" })).toBe(INITIAL);
    expect(nextPageLiveStage(DISCONNECTED, { type: "synced" })).toBe(DISCONNECTED);
  });

  it("counts a drop as a reconnect attempt", () => {
    for (const stage of LIVE) {
      expect(nextPageLiveStage(stage, { type: "dropped", attempt: 3 })).toEqual({
        kind: "reconnecting",
        attempt: 3,
      });
    }
  });

  it("ignores a drop at rest or terminal", () => {
    expect(nextPageLiveStage(INITIAL, { type: "dropped", attempt: 1 })).toBe(INITIAL);
    expect(nextPageLiveStage(DISCONNECTED, { type: "dropped", attempt: 9 })).toBe(DISCONNECTED);
  });

  it("lands terminal with the typed reason", () => {
    for (const stage of LIVE) {
      expect(nextPageLiveStage(stage, { type: "failed", reason: "auth" })).toEqual({
        kind: "disconnected",
        reason: "auth",
      });
    }
  });

  it("ignores a failure at rest or terminal", () => {
    expect(nextPageLiveStage(INITIAL, { type: "failed", reason: "auth" })).toBe(INITIAL);
    expect(nextPageLiveStage(DISCONNECTED, { type: "failed", reason: "force-close" })).toBe(DISCONNECTED);
  });

  it("resets from anywhere", () => {
    for (const stage of [INITIAL, ...LIVE, DISCONNECTED]) {
      expect(nextPageLiveStage(stage, { type: "reset" })).toEqual({ kind: "initial" });
    }
  });
});

describe("pageLiveSyncReport", () => {
  it("hides the badge only once synced", () => {
    expect(pageLiveSyncReport(SYNCED)).toBe("synced");
  });

  it("reports the fault badge once terminal", () => {
    expect(pageLiveSyncReport(DISCONNECTED)).toBe("lost");
  });

  it("reports transitional everywhere else", () => {
    for (const stage of [INITIAL, CONNECTING, AWAITING, RECONNECTING]) {
      expect(pageLiveSyncReport(stage)).toBe("syncing");
    }
  });
});

describe("pageLiveRetryDelayMs", () => {
  it("pins the retry budget", () => {
    expect(PAGE_LIVE_MAX_ATTEMPTS).toBe(8);
    expect(PAGE_LIVE_RETRY_BASE_MS).toBe(1000);
    expect(PAGE_LIVE_RETRY_CAP_MS).toBe(10_000);
    expect(PAGE_LIVE_CONNECT_TIMEOUT_MS).toBe(10_000);
  });

  it("doubles the beat per attempt", () => {
    expect(pageLiveRetryDelayMs(1)).toBe(1000);
    expect(pageLiveRetryDelayMs(2)).toBe(2000);
    expect(pageLiveRetryDelayMs(3)).toBe(4000);
    expect(pageLiveRetryDelayMs(4)).toBe(8000);
  });

  it("caps the beat", () => {
    expect(pageLiveRetryDelayMs(5)).toBe(10_000);
    expect(pageLiveRetryDelayMs(8)).toBe(10_000);
  });
});
