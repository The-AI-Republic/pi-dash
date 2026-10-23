// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver cycles row-menu methods (NEWFRONT-711,
// CYC-022, CYC-027, CYC-028, CYC-046): the update-dialog and peek openers,
// the delete-dialog readers, the failure shaping, the hover trigger, the
// new-tab opener, the archived zero-state detail, and the clipboard grant.
// A recording fake stands in for Playwright: each locator remembers the
// path of calls that built it, scripted answers stand in for the page,
// and a test asserts both which selector the driver used and what it
// does with the answer. Never about a real browser.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { descriptionAfterHeading } from "./modal-copy.js";
import { WebNewDriver } from "./web-new.js";

type Call = { path: string; method: string; args: unknown[] };

const CHAIN = ["first", "getByRole", "locator", "filter"];
const ACTIONS = ["count", "click", "innerText", "hover", "waitFor", "evaluate", "isVisible"];

function render(args: unknown[]): string {
  return args
    .map((arg) => {
      if (typeof arg === "function") return "<fn>";
      if (arg instanceof RegExp) return String(arg);
      if (typeof arg === "string") return JSON.stringify(arg);
      if (arg && typeof arg === "object") return JSON.stringify(arg);
      return String(arg);
    })
    .join(",");
}

const ROWS_PATH = `page.locator("div[data-testid=\\"cycle-row\\"]")`;
const MENUITEMS_PATH = `page.getByRole("menuitem")`;
const DIALOGS_PATH = `page.getByRole("dialog")`;
const FILTER_HAS_PATH = `.filter({"has":{}})`;

function rowPath(name: string): string {
  return `${ROWS_PATH}.filter(${JSON.stringify({ hasText: name })}).first()`;
}

function rowButtonPath(name: string, label: string): string {
  return `${rowPath(name)}.getByRole("button",${JSON.stringify({ name: label })})`;
}

const MENU_PICK_PATH = `${MENUITEMS_PATH}${FILTER_HAS_PATH}.first()`;
const DELETE_DIALOG_PATH = `${DIALOGS_PATH}${FILTER_HAS_PATH}.first()`;
const DELETE_HEADING_PATH = `${DELETE_DIALOG_PATH}.getByRole("heading",${JSON.stringify({ name: "Delete cycle" })}).first()`;
const ARCH_HEADING_PATH = `page.getByRole("heading",${JSON.stringify({ name: "No archived cycles yet" })}).first()`;
const ARCH_CARD_PATH = `${ARCH_HEADING_PATH}.locator("xpath=..")`;

interface ScriptedRoute {
  method: string;
  fulfilled: unknown[];
  continued: number;
}

class Recorder {
  readonly calls: Call[] = [];
  readonly routes: { pattern: string; handler: (route: unknown) => Promise<void> }[] = [];
  readonly granted: unknown[] = [];
  pageUrl = "https://app.test/acme/projects/pid/cycles";
  dialogPresent = true;
  dialogHeading = "Delete cycle";
  dialogBody = '"Cycle 7" will be permanently deleted.';
  headingRejects = false;
  bodyRejects = false;
  archivedHeadingPresent = true;
  archivedHeadingVisible = true;
  archivedHeadingText = "No archived cycles yet";
  archivedCardText = "No archived cycles yet Archived cycles will appear here.";
  archivedRejects = false;
  responsePredicate: ((response: unknown) => boolean) | null = null;
  responseThrows = false;
  popupUrl = "https://app.test/acme/projects/pid/cycles/cid";
  popupLoadThrows = false;
  waitedEvents: string[] = [];

  locator(path: string): Locator {
    const node: Record<string, unknown> = {};
    for (const method of CHAIN) {
      node[method] = (...args: unknown[]) => {
        this.calls.push({ path, method, args });
        return this.locator(`${path}.${method}(${render(args)})`);
      };
    }
    for (const method of ACTIONS) {
      node[method] = async (...args: unknown[]) => {
        const call = { path, method, args };
        this.calls.push(call);
        return this.respond(call);
      };
    }
    return node as unknown as Locator;
  }

  respond(call: Call): unknown {
    const { path, method, args } = call;
    if (method === "count") return this.count(path);
    if (method === "innerText") return this.text(path);
    if (method === "isVisible") return this.visible(path);
    if (method === "evaluate") {
      if (path === DELETE_HEADING_PATH && this.bodyRejects) throw new Error("detached");
      if (path === DELETE_HEADING_PATH) return this.dialogBody;
      throw new Error(`unexpected evaluate at ${path}`);
    }
    if (method === "waitFor") return undefined;
    if (method === "click" || method === "hover") return undefined;
    throw new Error(`unexpected action ${method} at ${path} with ${JSON.stringify(args)}`);
  }

  private count(path: string): number {
    if (path === DELETE_DIALOG_PATH) return this.dialogPresent ? 1 : 0;
    if (path === ARCH_HEADING_PATH) return this.archivedHeadingPresent ? 1 : 0;
    return 1;
  }

  private text(path: string): string {
    if (path === DELETE_HEADING_PATH) {
      if (this.headingRejects) throw new Error("detached");
      return this.dialogHeading;
    }
    if (path === ARCH_HEADING_PATH) {
      if (this.archivedRejects) throw new Error("detached");
      return this.archivedHeadingText;
    }
    if (path === ARCH_CARD_PATH) {
      if (this.archivedRejects) throw new Error("detached");
      return this.archivedCardText;
    }
    return "";
  }

  private visible(path: string): boolean {
    // isShown reads through one more .first(), so the visibility probe
    // lands there rather than on the heading path itself.
    if (path === `${ARCH_HEADING_PATH}.first()`) return this.archivedHeadingVisible;
    return true;
  }

  page(): Page {
    const node: Record<string, unknown> = {};
    for (const method of ["getByRole", "locator"]) {
      node[method] = (...args: unknown[]) => {
        this.calls.push({ path: "page", method, args });
        return this.locator(`page.${method}(${render(args)})`);
      };
    }
    node.url = () => this.pageUrl;
    node.route = async (pattern: string, handler: (route: unknown) => Promise<void>) => {
      this.calls.push({ path: "page", method: "route", args: [pattern] });
      this.routes.push({ pattern, handler });
    };
    node.waitForEvent = async (event: string) => {
      this.calls.push({ path: "page", method: "waitForEvent", args: [event] });
      this.waitedEvents.push(event);
      return {
        waitForLoadState: async () => {
          if (this.popupLoadThrows) throw new Error("gone");
          return undefined;
        },
        url: () => this.popupUrl,
      };
    };
    node.waitForResponse = async (predicate: (response: unknown) => boolean) => {
      this.calls.push({ path: "page", method: "waitForResponse", args: ["<fn>"] });
      this.responsePredicate = predicate;
      if (this.responseThrows) throw new Error("no write landed");
      return null;
    };
    node.context = () => ({
      grantPermissions: async (permissions: unknown) => {
        this.granted.push(permissions);
      },
    });
    return node as unknown as Page;
  }

  driver(): WebNewDriver {
    return new WebNewDriver(this.page());
  }

  callsFor(path: string, method: string): Call[] {
    return this.calls.filter((call) => call.path === path && call.method === method);
  }

  h5Lookups(): unknown[][] {
    return this.calls
      .filter((call) => call.path === "page" && call.method === "locator" && call.args[0] === "h5")
      .map((call) => call.args);
  }
}

function headingOptions(name: string): string {
  return JSON.stringify({ name });
}

describe("cyclesEditOpenUpdateDialog", () => {
  it("hovers the row, opens its menu, picks Edit, and waits for the dialog", async () => {
    const script = new Recorder();
    await script.driver().cyclesEditOpenUpdateDialog("Draft 1");
    expect(script.callsFor(rowPath("Draft 1"), "hover")).toHaveLength(1);
    expect(script.callsFor(rowButtonPath("Draft 1", "Cycle actions for Draft 1"), "click")).toHaveLength(1);
    expect(script.callsFor(`${MENUITEMS_PATH}.first()`, "waitFor")).toHaveLength(1);
    expect(script.h5Lookups()).toEqual([["h5", { hasText: "Edit" }]]);
    expect(script.callsFor(MENU_PICK_PATH, "click")).toHaveLength(1);
    const waits = script.callsFor(`page.getByRole("heading",${headingOptions("Update cycle")}).first()`, "waitFor");
    expect(waits).toHaveLength(1);
  });
});

describe("cyclesEditOpenPeek", () => {
  it("hovers the row and clicks its eye until the peek key lands", async () => {
    const script = new Recorder();
    script.pageUrl = "https://app.test/acme/projects/pid/cycles?peekCycle=cid";
    await script.driver().cyclesEditOpenPeek("Draft 1");
    expect(script.callsFor(rowPath("Draft 1"), "hover")).toHaveLength(1);
    expect(script.callsFor(rowButtonPath("Draft 1", "Quick look at Draft 1"), "click")).toHaveLength(1);
  });
});

describe("cyclesEditDeleteDialogText", () => {
  it("reads the heading and the copy beneath it", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesEditDeleteDialogText()).toEqual({
      heading: "Delete cycle",
      body: '"Cycle 7" will be permanently deleted.',
    });
    expect(script.callsFor(DELETE_DIALOG_PATH, "count")).not.toHaveLength(0);
    const reads = script.callsFor(DELETE_HEADING_PATH, "evaluate");
    expect(reads).toHaveLength(1);
    expect(reads[0]?.args[0]).toBe(descriptionAfterHeading);
  });

  it("reads blanks when the dialog detaches mid-read", async () => {
    const script = new Recorder();
    script.headingRejects = true;
    script.bodyRejects = true;
    expect(await script.driver().cyclesEditDeleteDialogText()).toEqual({ heading: "", body: "" });
  });

  it("returns null when no delete dialog ever mounts", async () => {
    const script = new Recorder();
    script.dialogPresent = false;
    const driver = script.driver() as unknown as { constructor: Record<string, unknown> };
    const previous = driver.constructor["WAIT_MS"];
    driver.constructor["WAIT_MS"] = 20;
    try {
      expect(await script.driver().cyclesEditDeleteDialogText()).toBeNull();
    } finally {
      driver.constructor["WAIT_MS"] = previous;
    }
  });
});

describe("cyclesEditDeleteConfirm", () => {
  it("clicks Delete and returns once the cycle write lands", async () => {
    const script = new Recorder();
    await script.driver().cyclesEditDeleteConfirm();
    const clicks = script.callsFor(
      `${DELETE_DIALOG_PATH}.getByRole("button",${JSON.stringify({ name: "Delete", exact: true })})`,
      "click"
    );
    expect(clicks).toHaveLength(1);
    expect(script.callsFor("page", "waitForResponse")).toHaveLength(1);
    const predicate = script.responsePredicate;
    expect(predicate).not.toBeNull();
    const response = (method: string, url: string): unknown => ({
      request: () => ({ method: () => method }),
      url: () => url,
    });
    expect(predicate?.(response("DELETE", "https://api.test/api/cycles/1/"))).toBe(true);
    expect(predicate?.(response("GET", "https://api.test/api/cycles/1/"))).toBe(false);
    expect(predicate?.(response("DELETE", "https://api.test/api/projects/"))).toBe(false);
  });

  it("still returns when no write lands", async () => {
    const script = new Recorder();
    script.responseThrows = true;
    await script.driver().cyclesEditDeleteConfirm();
    expect(script.callsFor("page", "waitForResponse")).toHaveLength(1);
  });
});

describe("cyclesEditDeleteCancel", () => {
  it("cancels and waits for the dialog to detach", async () => {
    const script = new Recorder();
    await script.driver().cyclesEditDeleteCancel();
    const clicks = script.callsFor(
      `${DELETE_DIALOG_PATH}.getByRole("button",${JSON.stringify({ name: "Cancel", exact: true })})`,
      "click"
    );
    expect(clicks).toHaveLength(1);
    const waits = script.callsFor(DELETE_DIALOG_PATH, "waitFor");
    expect(waits).toHaveLength(1);
    expect(waits[0]?.args[0]).toMatchObject({ state: "detached" });
  });
});

describe("cyclesEditFailNextDeleteWrite", () => {
  function scripted(): { script: Recorder; handler: (route: unknown) => Promise<void> } {
    return { script: new Recorder(), handler: async () => undefined };
  }

  it("refuses DELETEs with the permission body and passes the rest through", async () => {
    const { script } = scripted();
    await script.driver().cyclesEditFailNextDeleteWrite();
    expect(script.routes).toHaveLength(1);
    expect(script.routes[0]?.pattern).toBe("**/cycles/*/");
    const handler = script.routes[0]?.handler;
    if (!handler) throw new Error("route handler missing");

    const refused: ScriptedRoute = { method: "DELETE", fulfilled: [], continued: 0 };
    await handler({
      request: () => ({ method: () => refused.method }),
      fulfill: async (body: unknown) => {
        refused.fulfilled.push(body);
      },
      continue: async () => {
        refused.continued += 1;
      },
    });
    expect(refused.fulfilled).toEqual([
      {
        status: 403,
        contentType: "application/json",
        body: JSON.stringify({ error: "You don't have the required permissions." }),
      },
    ]);
    expect(refused.continued).toBe(0);

    const passed: ScriptedRoute = { method: "GET", fulfilled: [], continued: 0 };
    await handler({
      request: () => ({ method: () => passed.method }),
      fulfill: async (body: unknown) => {
        passed.fulfilled.push(body);
      },
      continue: async () => {
        passed.continued += 1;
      },
    });
    expect(passed.fulfilled).toEqual([]);
    expect(passed.continued).toBe(1);
  });
});

describe("cyclesRowMenuTriggerVisible", () => {
  it("reports the trigger's visibility without touching the menu", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesRowMenuTriggerVisible("Draft 1")).toBe(true);
    const probe = `${rowButtonPath("Draft 1", "Cycle actions for Draft 1")}.first()`;
    expect(script.callsFor(probe, "isVisible")).toHaveLength(1);
    expect(script.callsFor(rowButtonPath("Draft 1", "Cycle actions for Draft 1"), "click")).toHaveLength(0);
  });
});

describe("cyclesRowHover", () => {
  it("hovers the named row", async () => {
    const script = new Recorder();
    await script.driver().cyclesRowHover("Draft 1");
    expect(script.callsFor(rowPath("Draft 1"), "hover")).toHaveLength(1);
  });
});

describe("cyclesRowMenuOpenNewTab", () => {
  it("opens the menu, picks the new-tab entry, and resolves the popup address", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesRowMenuOpenNewTab("Draft 1")).toBe(
      "https://app.test/acme/projects/pid/cycles/cid"
    );
    expect(script.callsFor(rowPath("Draft 1"), "hover")).toHaveLength(1);
    expect(script.h5Lookups()).toEqual([["h5", { hasText: "Open in new tab" }]]);
    expect(script.callsFor(MENU_PICK_PATH, "click")).toHaveLength(1);
    expect(script.waitedEvents).toEqual(["popup"]);
  });

  it("still resolves when the popup never settles", async () => {
    const script = new Recorder();
    script.popupLoadThrows = true;
    expect(await script.driver().cyclesRowMenuOpenNewTab("Draft 1")).toBe(
      "https://app.test/acme/projects/pid/cycles/cid"
    );
  });
});

describe("cyclesArchivedEmptyDetail", () => {
  it("reads the supporting line beside the pinned heading", async () => {
    const script = new Recorder();
    expect(await script.driver().cyclesArchivedEmptyDetail()).toBe("Archived cycles will appear here.");
  });

  it("returns null when no zero-state renders", async () => {
    const script = new Recorder();
    script.archivedHeadingPresent = false;
    expect(await script.driver().cyclesArchivedEmptyDetail()).toBeNull();
  });

  it("returns null when the heading hides or the card holds only it", async () => {
    const script = new Recorder();
    script.archivedHeadingVisible = false;
    expect(await script.driver().cyclesArchivedEmptyDetail()).toBeNull();
    script.archivedHeadingVisible = true;
    script.archivedCardText = "No archived cycles yet";
    expect(await script.driver().cyclesArchivedEmptyDetail()).toBeNull();
  });

  it("reads nulls when the card detaches mid-read", async () => {
    const script = new Recorder();
    script.archivedRejects = true;
    expect(await script.driver().cyclesArchivedEmptyDetail()).toBeNull();
  });
});

describe("cyclesGrantClipboardAccess", () => {
  it("grants both clipboard permissions", async () => {
    const script = new Recorder();
    await script.driver().cyclesGrantClipboardAccess();
    expect(script.granted).toEqual([["clipboard-read", "clipboard-write"]]);
  });
});

describe("cycle menu driver timeouts", () => {
  it("leaves the shared wait budget untouched", () => {
    const budget = (WebNewDriver as unknown as Record<string, unknown>)["WAIT_MS"];
    expect(budget).toBe(30_000);
    expect(vi.isMockFunction(descriptionAfterHeading)).toBe(false);
  });
});
