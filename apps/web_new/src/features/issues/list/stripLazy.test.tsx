// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The rows mount the property strip from a chunk the route primes with its
// own module (NEWFRONT-911). These tests pin the contract the rows rely on:
// one shared request, no component before it settles, and the component
// after.
import { describe, expect, it, vi } from "vitest";

const strip = vi.hoisted(() => ({
  InlinePropertyStrip: (props: { marker: string }) => <div data-testid="strip">{props.marker}</div>,
}));

vi.mock("./InlinePropertyStrip.js", () => ({ InlinePropertyStrip: strip.InlinePropertyStrip }));

import { loadedInlineStrip, loadInlineStrip, primeInlineStrip } from "./stripLazy.js";

describe("row property strip chunk", () => {
  it("reports no component before the chunk has loaded", () => {
    expect(loadedInlineStrip()).toBeNull();
  });

  it("resolves the component after load and reuses the request", async () => {
    const first = loadInlineStrip();
    const second = loadInlineStrip();
    expect(second).toBe(first);
    await expect(first).resolves.toEqual({ default: strip.InlinePropertyStrip });
    expect(loadedInlineStrip()).toBe(strip.InlinePropertyStrip);
  });

  it("primes the request without awaiting it", async () => {
    primeInlineStrip();
    await expect(loadInlineStrip()).resolves.toEqual({ default: strip.InlinePropertyStrip });
    expect(loadedInlineStrip()).toBe(strip.InlinePropertyStrip);
  });
});
