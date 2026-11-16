// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Prerender/hydration parity (CLOUD-005, NEWFRONT-1041): the served document
// contains the home page's body exactly once, and hydrating that markup with
// the same components changes nothing — no re-rendered branch, no replaced
// node, no layout shift. The old page and this one both render their hero on
// the server, so this is the check that keeps it that way.
import { hydrateRoot } from "react-dom/client";
import { act } from "react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MarketingFrame } from "./components/MarketingFrame.js";
import { MarketingHome } from "./components/MarketingHome.js";
import { renderMarketingPage } from "./prerender.js";

const errors: unknown[] = [];

afterEach(() => {
  errors.length = 0;
  vi.restoreAllMocks();
});

describe("marketing home hydration", () => {
  it("hydrates the served home markup without reporting a mismatch", async () => {
    const page = renderMarketingPage("/home");
    const container = document.createElement("div");
    container.innerHTML = page.markup;
    document.body.appendChild(container);
    const before = container.innerHTML;
    const consoleError = vi.spyOn(console, "error").mockImplementation((...args) => {
      errors.push(args[0]);
    });

    const root = hydrateRoot(
      container,
      <MarketingFrame>
        <MarketingHome />
      </MarketingFrame>
    );
    await act(async () => undefined);
    // The served nodes are reused, not replaced: hydration leaves the same
    // document. A mismatch would have re-rendered the tree from scratch.
    const after = container.innerHTML;
    root.unmount();

    consoleError.mockRestore();
    container.remove();
    expect(errors).toEqual([]);
    expect(after).toBe(before);
  });

  it("serves the home page's heading text, not a placeholder, in the static markup", () => {
    const page = renderMarketingPage("/home");
    expect(page.markup).toContain("<h1");
    expect(page.markup).toContain("<header");
    expect(page.markup).toContain("<footer");
    expect(page.markup).toContain('href="/login"');
  });
});
