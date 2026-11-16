// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the notifications navigation read's shell settle
// (NEWFRONT-564, NTF-003/004). The dot is absent both in an empty inbox and
// on a document whose app never mounted, so the driver must only accept the
// first. These pin the states: a painted shell, a mounted page still
// painting, a dead document that paints after one reload, and one that
// never paints.
import type { Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface ShellState {
  /** Whether the inbox entry shows on the first visit. */
  entry: boolean;
  /** Whether the mount point has children on the first visit. */
  mounted?: boolean;
  /** Whether the inbox entry shows after a reload. */
  entryAfterReload?: boolean;
  /** Whether the entry paints on the second look, without a reload. */
  entryAfterWait?: boolean;
}

function shellPage(state: ShellState) {
  let reloaded = false;
  let entryLooks = 0;
  const context = { addCookies: vi.fn(async (_cookies: unknown) => undefined) };
  const page = {
    context: () => context,
    goto: vi.fn(async (_url: string, _options: unknown) => undefined),
    reload: vi.fn(async (_options: unknown) => {
      reloaded = true;
    }),
    waitForLoadState: vi.fn(async (_state: unknown) => undefined),
    waitForFunction: vi.fn(async () => undefined),
    waitForTimeout: vi.fn(async () => undefined),
    evaluate: vi.fn(async () => ""),
    url: () => "http://new.test/acme/projects/p1/issues",
    waitForResponse: vi.fn(async () => ({ ok: () => true })),
    locator: vi.fn((selector: string) => {
      if (selector === "#root") {
        return { evaluate: async () => state.mounted ?? false };
      }
      entryLooks += 1;
      const entryVisible = reloaded
        ? (state.entryAfterReload ?? state.entry)
        : state.entry || (entryLooks > 1 && (state.entryAfterWait ?? false));
      return {
        first: () => ({
          waitFor: async (_options: unknown) => {
            if (!entryVisible) throw new Error("not painted");
          },
        }),
        count: async () => (entryVisible ? 1 : 0),
      };
    }),
  };
  return { page: page as unknown as Page, context, pageStub: page };
}

describe("notificationsProjectNavBadge shell settle", () => {
  it("reads the dot once the shell has painted the inbox entry", async () => {
    const stub = shellPage({ entry: true, mounted: true });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.notificationsProjectNavBadge("acme", "p1", [])).resolves.toBeNull();
    expect(stub.pageStub.reload).not.toHaveBeenCalled();
  });

  it("reloads one unpainted shell and reads the second paint", async () => {
    const stub = shellPage({ entry: false, mounted: false, entryAfterReload: true });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.notificationsProjectNavBadge("acme", "p1", [])).resolves.toBeNull();
    expect(stub.pageStub.reload).toHaveBeenCalledTimes(1);
  });

  it("throws when the shell never paints instead of reporting an empty inbox", async () => {
    const stub = shellPage({ entry: false, mounted: false, entryAfterReload: false });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.notificationsProjectNavBadge("acme", "p1", [])).rejects.toThrow(
      /never painted the inbox entry/
    );
    expect(stub.pageStub.reload).toHaveBeenCalledTimes(1);
  });

  it("waits out a mounted page that is still painting instead of reloading it", async () => {
    // A mounted app is only slow: reloading it would restart the entry fetch
    // and turn a slow paint into a reload loop.
    const stub = shellPage({ entry: false, mounted: true, entryAfterWait: true });
    const driver = new WebNewDriver(stub.page);

    await expect(driver.notificationsProjectNavBadge("acme", "p1", [])).resolves.toBeNull();
    expect(stub.pageStub.reload).not.toHaveBeenCalled();
  });
});

describe("goToPath reload recovery", () => {
  function pathPage(children: number) {
    const goto = vi.fn(async (_url: string, _options: unknown) => undefined);
    const reload = vi.fn(async (_options: unknown) => undefined);
    const waitForLoadState = vi.fn(async (_state: unknown) => undefined);
    const page = {
      goto,
      reload,
      waitForLoadState,
      locator: vi.fn((_selector: string) => ({ evaluate: async () => children > 0 })),
    };
    return { page: page as unknown as Page, goto, reload };
  }

  it("leaves a mounted page alone", async () => {
    const stub = pathPage(1);

    await new WebNewDriver(stub.page).goToPath("/acme/notifications/");

    expect(stub.goto).toHaveBeenCalledWith("/acme/notifications/", { waitUntil: "domcontentloaded", timeout: 120_000 });
    expect(stub.reload).not.toHaveBeenCalled();
  });

  it("reloads one empty mount point so a dead module graph is replaced", async () => {
    const stub = pathPage(0);

    await new WebNewDriver(stub.page).goToPath("/acme/notifications/");

    expect(stub.reload).toHaveBeenCalledWith({ waitUntil: "domcontentloaded", timeout: 120_000 });
  });
});
