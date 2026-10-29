// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Chunk-load retry (NEWFRONT-772, EXP-023): first success wins without
// retrying, transient failures retry, and a persistently broken chunk
// rejects with its last error after the bounded attempts.
import { describe, expect, it, vi } from "vitest";

import { CHUNK_LOAD_ATTEMPTS, loadWithRetry } from "./chunkRetry.js";

describe("loadWithRetry", () => {
  it("resolves without retrying when the first load succeeds", async () => {
    const load = vi.fn(async () => undefined);
    await expect(loadWithRetry(load, 4)).resolves.toBeUndefined();
    expect(load).toHaveBeenCalledTimes(1);
  });

  it("retries transient failures and resolves on the first success", async () => {
    const load = vi
      .fn(async () => undefined)
      .mockRejectedValueOnce(new Error("blip one"))
      .mockRejectedValueOnce(new Error("blip two"));
    await expect(loadWithRetry(load, 4)).resolves.toBeUndefined();
    expect(load).toHaveBeenCalledTimes(3);
  });

  it("rejects with the last error after exhausting the attempts", async () => {
    const last = new Error("still broken");
    const load = vi
      .fn(async () => undefined)
      .mockRejectedValueOnce(new Error("blip one"))
      .mockRejectedValue(last);
    await expect(loadWithRetry(load, 3)).rejects.toBe(last);
    expect(load).toHaveBeenCalledTimes(3);
  });

  it("bounds the route's chunk load to more than one attempt", () => {
    expect(CHUNK_LOAD_ATTEMPTS).toBeGreaterThanOrEqual(2);
  });
});
