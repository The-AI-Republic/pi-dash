// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { isEmbeddedWindow, readSpaceEmbedded, useSpaceEmbedded } from "./spaceEmbed.js";

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("isEmbeddedWindow", () => {
  it("reports a top-level window as not embedded", () => {
    const top = {};
    expect(isEmbeddedWindow({ self: top, top })).toBe(false);
  });

  it("reports a framed window as embedded", () => {
    expect(isEmbeddedWindow({ self: {}, top: {} })).toBe(true);
  });

  it("reports a missing window as not embedded", () => {
    expect(isEmbeddedWindow(undefined)).toBe(false);
  });
});

describe("readSpaceEmbedded", () => {
  it("reads the live window", () => {
    expect(readSpaceEmbedded()).toBe(false);
  });

  it("reports no frame without a window", () => {
    vi.stubGlobal("window", undefined);
    expect(readSpaceEmbedded()).toBe(false);
  });
});

describe("useSpaceEmbedded", () => {
  it("settles on the first render", () => {
    const { result } = renderHook(() => useSpaceEmbedded());
    expect(result.current).toBe(false);
  });
});
