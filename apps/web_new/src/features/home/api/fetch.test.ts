// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

import { logHomeFetch } from "./fetch.js";

afterEach(() => {
  vi.restoreAllMocks();
});

describe("logHomeFetch", () => {
  it("passes successes through untouched", async () => {
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    await expect(logHomeFetch("recents", async () => [1])).resolves.toEqual([1]);
    expect(errorSpy).not.toHaveBeenCalled();
  });

  it("logs failures with their label and rethrows", async () => {
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const failure = new Error("denied");
    await expect(logHomeFetch("recents/acme", async () => Promise.reject(failure))).rejects.toBe(failure);
    expect(errorSpy).toHaveBeenCalledWith("[web_new] home fetch failed: recents/acme", failure);
  });
});
