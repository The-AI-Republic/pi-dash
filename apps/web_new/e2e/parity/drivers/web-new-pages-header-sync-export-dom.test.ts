// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// DOM coverage for the three WebNewDriver header reads that walk text
// nodes inside page.evaluate (NEWFRONT-923, PAGE-061-064, 074): the
// oversized strip, the sync badge text and the toast stack. Each
// browser-serialized predicate is captured from the driver and invoked
// in this document against the shipped hooks, so the test fails when a
// selector stops matching the markup.
import type { Page } from "@playwright/test";
import { afterEach, describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new";

afterEach(() => {
  document.body.innerHTML = "";
});

/** A page that runs each evaluated predicate inside this jsdom document. */
function evaluatingPage(): Page {
  return { evaluate: async (fn: () => unknown) => fn() } as unknown as Page;
}

describe("oversized strip", () => {
  it("reads null when the strip is absent", async () => {
    const driver = new WebNewDriver(evaluatingPage());

    await expect(driver.pagesHeaderOversizedStrip()).resolves.toBeNull();
  });

  it("joins the strip text across nodes", async () => {
    document.body.innerHTML =
      '<div data-testid="page-oversized-strip"><h2>Page too large to sync</h2>' +
      "<p>Split content across pages.</p></div>";
    const driver = new WebNewDriver(evaluatingPage());

    await expect(driver.pagesHeaderOversizedStrip()).resolves.toBe(
      "Page too large to sync Split content across pages."
    );
  });

  it("reads null for a whitespace-only strip", async () => {
    document.body.innerHTML = '<div data-testid="page-oversized-strip">   </div>';
    const driver = new WebNewDriver(evaluatingPage());

    await expect(driver.pagesHeaderOversizedStrip()).resolves.toBeNull();
  });

  it("caps the scan and slices long strips", async () => {
    document.body.innerHTML = `<div data-testid="page-oversized-strip">${"<span>0123456789</span>".repeat(50)}</div>`;
    const driver = new WebNewDriver(evaluatingPage());

    const text = await driver.pagesHeaderOversizedStrip();
    expect(text).not.toBeNull();
    expect(text?.length).toBe(240);
    expect(text?.startsWith("0123456789 0123456789")).toBe(true);
  });
});

describe("sync badge text", () => {
  it("reads null when the header is absent", async () => {
    const driver = new WebNewDriver(evaluatingPage());

    await expect(driver.pagesHeaderSyncBadgeText()).resolves.toBeNull();
  });

  it("prefers the connection-lost fault over a transition", async () => {
    document.body.innerHTML =
      '<header data-testid="page-detail-header"><span>Syncing...</span><span>Connection lost</span></header>';
    const driver = new WebNewDriver(evaluatingPage());

    await expect(driver.pagesHeaderSyncBadgeText()).resolves.toBe("Connection lost");
  });

  it("reads a lone transition and null when the header is quiet", async () => {
    document.body.innerHTML = '<header data-testid="page-detail-header"><span>Syncing...</span></header>';
    const syncing = new WebNewDriver(evaluatingPage());
    await expect(syncing.pagesHeaderSyncBadgeText()).resolves.toBe("Syncing...");

    document.body.innerHTML = '<header data-testid="page-detail-header"><span>Saved</span></header>';
    const quiet = new WebNewDriver(evaluatingPage());
    await expect(quiet.pagesHeaderSyncBadgeText()).resolves.toBeNull();
  });

  it("stops scanning past its node budget", async () => {
    document.body.innerHTML =
      `<header data-testid="page-detail-header">${"<span>filler</span>".repeat(110)}` +
      "<span>Syncing...</span></header>";
    const driver = new WebNewDriver(evaluatingPage());

    await expect(driver.pagesHeaderSyncBadgeText()).resolves.toBeNull();
  });
});

describe("toast text", () => {
  it("reads null when the stack is absent", async () => {
    const driver = new WebNewDriver(evaluatingPage());

    await expect(driver.pagesHeaderToastText()).resolves.toBeNull();
  });

  it("joins toast text across nodes", async () => {
    document.body.innerHTML =
      '<div data-testid="toast-stack"><div role="status">Saved</div><div role="status">Page exported</div></div>';
    const driver = new WebNewDriver(evaluatingPage());

    await expect(driver.pagesHeaderToastText()).resolves.toBe("Saved Page exported");
  });

  it("reads null for an empty stack", async () => {
    document.body.innerHTML = '<div data-testid="toast-stack">  </div>';
    const driver = new WebNewDriver(evaluatingPage());

    await expect(driver.pagesHeaderToastText()).resolves.toBeNull();
  });

  it("slices long toasts", async () => {
    document.body.innerHTML = `<div data-testid="toast-stack"><div role="status">${"y".repeat(600)}</div></div>`;
    const driver = new WebNewDriver(evaluatingPage());

    const text = await driver.pagesHeaderToastText();
    expect(text?.length).toBe(500);
  });

  it("stops scanning past its node budget", async () => {
    document.body.innerHTML = `<div data-testid="toast-stack">${"<span>x</span>".repeat(130)}</div>`;
    const driver = new WebNewDriver(evaluatingPage());

    const text = await driver.pagesHeaderToastText();
    expect(text?.length).toBe(120 + 119);
  });
});
