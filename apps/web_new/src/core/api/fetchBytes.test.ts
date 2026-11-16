// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Platform-transport byte fetching: answers resolve to bytes, misses reject.
import { afterEach, describe, expect, it, vi } from "vitest";

import { fetchBytes, fetchPageBytes } from "./fetchBytes.js";

afterEach(() => {
  vi.unstubAllGlobals();
});

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

describe("fetchPageBytes", () => {
  it("fetches with credentials through the page transport", async () => {
    const payload = new Uint8Array([137, 80, 78, 71]);
    const fetchMock = vi.fn(async () => ({ ok: true, arrayBuffer: async () => payload.buffer }) as Response);
    vi.stubGlobal("fetch", fetchMock);
    await expect(fetchPageBytes("https://api.test/api/assets/v2/a/")).resolves.toEqual(payload);
    expect(fetchMock).toHaveBeenCalledWith(
      "https://api.test/api/assets/v2/a/",
      expect.objectContaining({ credentials: "include" })
    );
  });

  it("rejects when the address is missing", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({ ok: false, arrayBuffer: async () => new ArrayBuffer(0) }) as Response)
    );
    await expect(fetchPageBytes("https://example.com/gone.png")).rejects.toThrow("Could not load");
  });
});
