// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Chunk-load retry (NEWFRONT-772, EXP-023/024): the first attempt runs
// immediately, transient failures retry spaced apart, and a persistently
// broken chunk rejects with its last error after the bounded attempts.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { CHUNK_LOAD_ATTEMPTS, CHUNK_LOAD_RETRY_DELAY_MS, loadWithRetry } from "./chunkRetry.js";

describe("loadWithRetry", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("resolves without waiting when the first load succeeds", async () => {
    const load = vi.fn(async () => undefined);
    await expect(loadWithRetry(load, 4, 1000)).resolves.toBeUndefined();
    expect(load).toHaveBeenCalledTimes(1);
  });

  it("spaces retries apart and resolves on the first success", async () => {
    const load = vi
      .fn(async () => undefined)
      .mockRejectedValueOnce(new Error("blip one"))
      .mockRejectedValueOnce(new Error("blip two"));
    const pending = loadWithRetry(load, 4, 1000);
    const assertion = expect(pending).resolves.toBeUndefined();
    await vi.advanceTimersByTimeAsync(999);
    expect(load).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(load).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(1000);
    await assertion;
    expect(load).toHaveBeenCalledTimes(3);
  });

  it("rejects with the last error after exhausting the attempts", async () => {
    const last = new Error("still broken");
    const load = vi
      .fn(async () => undefined)
      .mockRejectedValueOnce(new Error("blip one"))
      .mockRejectedValue(last);
    const pending = loadWithRetry(load, 3, 1000);
    const assertion = expect(pending).rejects.toBe(last);
    await vi.advanceTimersByTimeAsync(5000);
    await assertion;
    expect(load).toHaveBeenCalledTimes(3);
  });

  it("bounds the route's chunk load inside the driver's settle window", () => {
    expect(CHUNK_LOAD_ATTEMPTS).toBeGreaterThanOrEqual(2);
    expect(CHUNK_LOAD_RETRY_DELAY_MS).toBeGreaterThan(0);
    expect(CHUNK_LOAD_ATTEMPTS * CHUNK_LOAD_RETRY_DELAY_MS).toBeLessThanOrEqual(30_000);
  });
});
