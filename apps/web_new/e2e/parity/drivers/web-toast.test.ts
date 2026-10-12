// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Regression tests for the WebDriver last-toast readers (NEWFRONT-404,
// NEWFRONT-407): the old app renders its toast roots newest-prepended
// (one global Base UI stack, and the manager prepends each new toast),
// so the readers must honor their "most recently shown" contracts from
// the front of DOM order. A stub Page stands in for Playwright; the
// readers only use locator, getByText, getByRole, count, nth, first,
// last, innerText, isVisible, and evaluate.
import type { Page } from "@playwright/test";
import { describe, expect, it } from "vitest";
import { WebDriver } from "./web";

/** A driver whose toast roots carry `texts` in DOM order (index 0 first). */
function driverWithToasts(texts: string[]): WebDriver {
  const at = (index: number) => ({ innerText: async () => texts[index] ?? "" });
  const roots = {
    count: async () => texts.length,
    nth: (index: number) => at(index),
    first: () => at(0),
    last: () => at(texts.length - 1),
  };
  const page = { locator: () => roots };
  return new WebDriver(page as unknown as Page);
}

type LastToastReader = "rulesLastToast" | "runnerChatLastToast" | "assistantLastToast";

const readers: LastToastReader[] = ["rulesLastToast", "runnerChatLastToast", "assistantLastToast"];

describe("last-toast readers", () => {
  it.each(readers)("returns the newest toast when two overlap (newest first in DOM): %s", async (reader) => {
    const driver = driverWithToasts(["Error!\nState name is already taken", "Success!\nState created successfully"]);
    await expect(driver[reader]()).resolves.toEqual({
      title: "Error!",
      message: "State name is already taken",
    });
  });

  it.each(readers)("returns the single visible toast: %s", async (reader) => {
    const driver = driverWithToasts(["Success!\nState created successfully"]);
    await expect(driver[reader]()).resolves.toEqual({
      title: "Success!",
      message: "State created successfully",
    });
  });

  it.each(readers)("skips empty roots: %s", async (reader) => {
    const driver = driverWithToasts(["   ", "Success!\nState created successfully"]);
    await expect(driver[reader]()).resolves.toEqual({
      title: "Success!",
      message: "State created successfully",
    });
  });

  it.each(readers)("returns null when no toast is visible: %s", async (reader) => {
    const driver = driverWithToasts([]);
    await expect(driver[reader]()).resolves.toBeNull();
  });

  it.each(readers)("returns null when every root is empty: %s", async (reader) => {
    const driver = driverWithToasts(["", "  \n "]);
    await expect(driver[reader]()).resolves.toBeNull();
  });
});

type NotificationsReader = "boardLastToast" | "modulesLastToast";

const notificationsReaders: NotificationsReader[] = ["boardLastToast", "modulesLastToast"];

describe("Notifications-region last-toast readers", () => {
  it.each(notificationsReaders)(
    "returns the newest toast when two overlap (newest first in DOM): %s",
    async (reader) => {
      const driver = driverWithToasts(["Error!\nCannot move the work item", "Success!\nWork item updated"]);
      await expect(driver[reader]()).resolves.toEqual({
        title: "Error!",
        message: "Cannot move the work item",
      });
    }
  );

  it.each(notificationsReaders)("returns the single visible toast: %s", async (reader) => {
    const driver = driverWithToasts(["Success!\nModule updated successfully."]);
    await expect(driver[reader]()).resolves.toEqual({
      title: "Success!",
      message: "Module updated successfully.",
    });
  });

  it.each(notificationsReaders)("returns null when no toast is visible: %s", async (reader) => {
    const driver = driverWithToasts([]);
    await expect(driver[reader]()).resolves.toBeNull();
  });

  it.each(notificationsReaders)("returns null when the newest root is empty: %s", async (reader) => {
    const driver = driverWithToasts(["  \n "]);
    await expect(driver[reader]()).resolves.toBeNull();
  });
});

/**
 * A driver with toast roots plus in-flow live-region nodes. `roots` are
 * the portaled toast roots in DOM order (index 0 first, newest first);
 * `status`/`alert` are in-flow nodes, which sort before the portaled
 * roots in DOM order.
 */
/** Marks a stub node whose innerText read rejects (an auto-dismiss race). */
const REJECTING_READ = "\u0000rejecting-read";

function homeDriverWithNodes(roots: string[], status: string[], alert: string[]): WebDriver {
  const group = (texts: string[]) => {
    const textAt = async (index: number): Promise<string> => {
      const value = texts[index] ?? "";
      if (value === REJECTING_READ) throw new Error("node detached");
      return value;
    };
    return {
      count: async () => texts.length,
      first: () => ({ innerText: () => textAt(0) }),
      last: () => ({ innerText: () => textAt(texts.length - 1) }),
    };
  };
  const page = {
    locator: (selector: string) =>
      selector.includes('[role="status"]')
        ? group([...status, ...alert, ...roots]) // Pre-fix union selector: DOM order.
        : group(roots), // Post-fix toast-roots read.
    getByRole: (role: string) => group(role === "status" ? status : alert),
  };
  return new WebDriver(page as unknown as Page);
}

describe("homeLastToast", () => {
  it("returns the newest toast when two overlap (newest first in DOM)", async () => {
    const driver = homeDriverWithNodes(["Error!\nQuick link not updated", "Success!\nWidget reordered"], [], []);
    await expect(driver.homeLastToast()).resolves.toEqual({
      title: "Error!",
      message: "Quick link not updated",
    });
  });

  it("returns the newest toast when a static alert is also present", async () => {
    const driver = homeDriverWithNodes(
      ["Error!\nQuick link not updated", "Success!\nWidget reordered"],
      [],
      ["Form invalid"]
    );
    await expect(driver.homeLastToast()).resolves.toEqual({
      title: "Error!",
      message: "Quick link not updated",
    });
  });

  it("returns the single toast when a static alert is also present", async () => {
    const driver = homeDriverWithNodes(["Error!\nQuick link not updated"], [], ["Form invalid"]);
    await expect(driver.homeLastToast()).resolves.toEqual({
      title: "Error!",
      message: "Quick link not updated",
    });
  });

  it("falls back to a status node when no toast shows", async () => {
    const driver = homeDriverWithNodes([], ["Saved"], []);
    await expect(driver.homeLastToast()).resolves.toEqual({ title: "Saved", message: "" });
  });

  it("falls back to an alert node when no toast or status shows", async () => {
    const driver = homeDriverWithNodes([], [], ["Warning\nCheck this"]);
    await expect(driver.homeLastToast()).resolves.toEqual({ title: "Warning", message: "Check this" });
  });

  it("falls through an empty toast root to the status fallback", async () => {
    const driver = homeDriverWithNodes(["  "], ["Saved"], []);
    await expect(driver.homeLastToast()).resolves.toEqual({ title: "Saved", message: "" });
  });

  it("falls through an empty status node to the alert fallback", async () => {
    const driver = homeDriverWithNodes([], ["  "], ["Warning"]);
    await expect(driver.homeLastToast()).resolves.toEqual({ title: "Warning", message: "" });
  });

  it("returns null when no toast, status, or alert shows", async () => {
    const driver = homeDriverWithNodes([], [], []);
    await expect(driver.homeLastToast()).resolves.toBeNull();
  });

  it("returns null when every node is empty", async () => {
    const driver = homeDriverWithNodes(["  "], [], [" \n "]);
    await expect(driver.homeLastToast()).resolves.toBeNull();
  });

  it("falls through a rejecting toast root to the status fallback", async () => {
    const driver = homeDriverWithNodes([REJECTING_READ], ["Saved"], []);
    await expect(driver.homeLastToast()).resolves.toEqual({ title: "Saved", message: "" });
  });

  it("falls through rejecting toast and status nodes to the alert fallback", async () => {
    const driver = homeDriverWithNodes([REJECTING_READ], [REJECTING_READ], ["Warning\nCheck this"]);
    await expect(driver.homeLastToast()).resolves.toEqual({ title: "Warning", message: "Check this" });
  });

  it("returns null when every read rejects", async () => {
    const driver = homeDriverWithNodes([REJECTING_READ], [REJECTING_READ], [REJECTING_READ]);
    await expect(driver.homeLastToast()).resolves.toBeNull();
  });
});

/** A driver whose exact-"Error!" title matches carry `texts` in DOM order (index 0 first). */
function runnersDriverWithErrors(texts: string[]): WebDriver {
  const titles = {
    count: async () => texts.length,
    first: () => ({ evaluate: async () => texts[0] ?? "" }),
    last: () => ({ evaluate: async () => texts[texts.length - 1] ?? "" }),
  };
  const page = { getByText: () => titles };
  return new WebDriver(page as unknown as Page);
}

describe("runnersLastToast", () => {
  it("returns the newest Error toast when two overlap (newest first in DOM)", async () => {
    const driver = runnersDriverWithErrors(["Error!\nCould not create the pod.", "Error!\nFailed to delete runner"]);
    await expect(driver.runnersLastToast()).resolves.toBe("Error!\nCould not create the pod.");
  });

  it("returns the single Error toast", async () => {
    const driver = runnersDriverWithErrors(["Error!\nFailed to delete runner"]);
    await expect(driver.runnersLastToast()).resolves.toBe("Error!\nFailed to delete runner");
  });

  it("returns null when no Error toast shows", async () => {
    const driver = runnersDriverWithErrors([]);
    await expect(driver.runnersLastToast()).resolves.toBeNull();
  });

  it("returns null when the newest match is empty", async () => {
    const driver = runnersDriverWithErrors([""]);
    await expect(driver.runnersLastToast()).resolves.toBeNull();
  });
});

/** A driver whose toast roots carry `texts` in DOM order (index 0 first, newest first). */
function schedulerDriverWithToasts(texts: string[]): WebDriver {
  const roots = {
    count: async () => texts.length,
    nth: (index: number) => ({
      isVisible: async () => true,
      innerText: async () => texts[index] ?? "",
    }),
  };
  const page = { locator: () => roots };
  return new WebDriver(page as unknown as Page);
}

describe("schedulerVisibleToasts", () => {
  it("lists visible toasts newest first (first-to-last over the prepended stack)", async () => {
    const driver = schedulerDriverWithToasts([
      "1 project failed\nBulk Beta could not install",
      "Installed on 1 project\nBulk Alpha now uses the configured schedule",
    ]);
    await expect(driver.schedulerVisibleToasts()).resolves.toEqual([
      { title: "1 project failed", message: "Bulk Beta could not install" },
      { title: "Installed on 1 project", message: "Bulk Alpha now uses the configured schedule" },
    ]);
  });

  it("lists the single visible toast", async () => {
    const driver = schedulerDriverWithToasts(["Select a project\nPick at least one project"]);
    await expect(driver.schedulerVisibleToasts()).resolves.toEqual([
      { title: "Select a project", message: "Pick at least one project" },
    ]);
  });

  it("lists no toasts when none shows", async () => {
    const driver = schedulerDriverWithToasts([]);
    await expect(driver.schedulerVisibleToasts()).resolves.toEqual([]);
  });
});
