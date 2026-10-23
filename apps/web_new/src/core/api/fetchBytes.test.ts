// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Platform-transport byte fetching: answers resolve to bytes, misses reject.
import { describe, expect, it, vi } from "vitest";

import { fetchBytes } from "./fetchBytes.js";

describe("fetchBytes", () => {
  it("reads the answer bytes through the given transport", async () => {
    const payload = new Uint8Array([137, 80, 78, 71]);
    const fetchImpl = vi.fn(async () => ({ ok: true, arrayBuffer: async () => payload.buffer }) as Response);
    await expect(fetchBytes("https://example.com/a.png", fetchImpl)).resolves.toEqual(payload);
    expect(fetchImpl).toHaveBeenCalledWith("https://example.com/a.png");
  });

  it("rejects when the address is missing", async () => {
    const fetchImpl = vi.fn(async () => ({ ok: false, arrayBuffer: async () => new ArrayBuffer(0) }) as Response);
    await expect(fetchBytes("https://example.com/gone.png", fetchImpl)).rejects.toThrow("Could not load");
  });
});
