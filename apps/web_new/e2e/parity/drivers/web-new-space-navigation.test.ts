// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the new app's public-board navigation and failure hooks
// (NEWFRONT-753, SPC-001/002/003/004/009). The scenarios address the old
// app's mount-free paths; the driver mounts them under /spaces and reports
// positions the same way.
import type { Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface FakePage {
  goto: ReturnType<typeof vi.fn>;
  waitForLoadState: ReturnType<typeof vi.fn>;
  url: ReturnType<typeof vi.fn>;
  route: ReturnType<typeof vi.fn>;
  unrouteAll: ReturnType<typeof vi.fn>;
  getByTestId: ReturnType<typeof vi.fn>;
  locator: ReturnType<typeof vi.fn>;
}

function fakePage(url = "http://new.test/spaces/issues/anchor-1/"): {
  page: FakePage;
  driver: WebNewDriver;
} {
  const page: FakePage = {
    goto: vi.fn(async () => undefined),
    waitForLoadState: vi.fn(async () => undefined),
    url: vi.fn(() => url),
    route: vi.fn(async () => undefined),
    unrouteAll: vi.fn(async () => undefined),
    // The landing opener waits for a settled card or guidance view; the
    // stub reports settled immediately.
    getByTestId: vi.fn(() => ({ isVisible: async () => true })),
    // The board opener recovers from the error screen only when no rows
    // render; the stub reports rows so no reload is attempted.
    locator: vi.fn(() => ({ count: async () => 1 })),
  };
  return { page, driver: new WebNewDriver(page as unknown as Page) };
}

describe("WebNewDriver space navigation", () => {
  it("reports board and landing positions without the /spaces mount", async () => {
    const board = fakePage("http://new.test/spaces/issues/anchor-1/?board=kanban&peekId=p1");
    await expect(board.driver.currentPath()).resolves.toBe("/issues/anchor-1/?board=kanban&peekId=p1");
    const landing = fakePage("http://new.test/spaces/?next_path=%2Fissues%2Fanchor-1%2F");
    await expect(landing.driver.currentPath()).resolves.toBe("/?next_path=%2Fissues%2Fanchor-1%2F");
    const workspace = fakePage("http://new.test/acme/projects/p1/issues");
    await expect(workspace.driver.currentPath()).resolves.toBe("/acme/projects/p1/issues");
  });

  it("opens boards, landing and legacy links under the public subtree", async () => {
    const { page, driver } = fakePage();
    await driver.spaceOpenBoard("anchor-1", "?board=list");
    await driver.spaceOpenLanding();
    await driver.spaceOpenLegacyLink("acme", "p1");
    expect(page.goto.mock.calls.map((call) => call[0])).toEqual([
      "/spaces/issues/anchor-1?board=list",
      "/spaces/",
      "/spaces/acme/p1/",
    ]);
  });

  it("mounts arbitrary scenario paths under /spaces", async () => {
    const { page, driver } = fakePage();
    for (const path of ["/", "", "/spaces", "/spaces/known/", "/unknown/path", "relative"]) {
      await driver.spaceAccessOpenPath(path);
    }
    expect(page.goto.mock.calls.map((call) => call[0])).toEqual([
      "/spaces/",
      "/spaces/",
      "/spaces",
      "/spaces/known/",
      "/spaces/unknown/path",
      "/spaces/relative",
    ]);
  });

  it("installs and removes the board failure routes", async () => {
    const { page, driver } = fakePage();
    await driver.spaceAccessFailBoardSettings(503);
    await driver.spaceAccessFailBoardIssues();
    expect(page.route).toHaveBeenCalledTimes(2);
    expect(page.route.mock.calls[0]?.[0]).toBe("**/api/public/anchor/*/settings/");
    expect(page.route.mock.calls[1]?.[0]).toBeInstanceOf(RegExp);
    await driver.spaceAccessClearFailures();
    expect(page.unrouteAll).toHaveBeenCalledWith({ behavior: "wait" });
  });
});
