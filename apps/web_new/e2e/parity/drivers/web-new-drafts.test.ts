// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver draft methods (NEWFRONT-643,
// ISS-137–141; list-surface reads added by NEWFRONT-782, DRAFT-001–003,
// 022–024, 026, 027). A recording fake stands in for Playwright: each locator
// remembers the path of calls that built it, so a test can assert both
// which selector the driver used and what it does with the answer.

import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new.js";

type Call = { path: string; method: string; args: unknown[] };
type Respond = (call: Call) => unknown;

const CHAIN = ["filter", "first", "last", "getByRole", "getByText", "getByPlaceholder", "locator", "getByTestId"];
const ACTIONS = [
  "count",
  "isVisible",
  "isDisabled",
  "click",
  "dblclick",
  "dispatchEvent",
  "waitFor",
  "textContent",
  "allTextContents",
  "innerText",
  "allInnerTexts",
  "getAttribute",
  "isDisabled",
  "inputValue",
];

function render(args: unknown[]): string {
  return args
    .map((arg) => {
      if (typeof arg === "string") return JSON.stringify(arg);
      if (arg && typeof arg === "object") {
        const record = arg as Record<string, unknown>;
        // A `has` locator is a filter target; keep its chain readable.
        if (record.toString && String(record.toString).includes("locator")) return "<has>";
        // Keep accessible-name patterns readable instead of `"name":{}`.
        return JSON.stringify(record, (_key, value: unknown) => (value instanceof RegExp ? String(value) : value));
      }
      return String(arg);
    })
    .join(",");
}

class Recorder {
  readonly calls: Call[] = [];
  // Unstated paths answer with their type's quiet value, so a test only
  // has to describe the paths it cares about.
  respond: Respond = ({ method }) => {
    if (method === "count") return 0;
    if (method === "allTextContents" || method === "allInnerTexts") return [];
    if (method === "textContent" || method === "inputValue" || method === "title" || method === "innerText") {
      return "";
    }
    if (method === "getAttribute") return null;
    if (method === "isDisabled" || method === "isVisible") return false;
    return undefined;
  };

  locator(path: string): Locator {
    const node: Record<string, unknown> = {};
    for (const method of CHAIN) {
      node[method] = (...args: unknown[]) => this.locator(`${path}.${method}(${render(args)})`);
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

  page(): Page {
    const node: Record<string, unknown> = {};
    for (const method of ["getByRole", "getByText", "getByPlaceholder", "locator", "getByTestId"]) {
      node[method] = (...args: unknown[]) => this.locator(`page.${method}(${render(args)})`);
    }
    node.goto = async (url: string) => {
      this.calls.push({ path: "page.goto", method: "goto", args: [url] });
    };
    node.title = async () => {
      const call = { path: "page.title", method: "title", args: [] as unknown[] };
      this.calls.push(call);
      return this.respond(call);
    };
    node.route = async (...args: unknown[]) => {
      this.calls.push({ path: "page.route", method: "route", args });
    };
    node.unroute = async (...args: unknown[]) => {
      this.calls.push({ path: "page.unroute", method: "unroute", args });
    };
    node.on = (event: string, listener: unknown) => {
      this.calls.push({ path: "page.on", method: "on", args: [event, listener] });
    };
    node.off = (event: string, listener: unknown) => {
      this.calls.push({ path: "page.off", method: "off", args: [event, listener] });
    };
    node.evaluate = async (...args: unknown[]) => {
      this.calls.push({ path: "page.evaluate", method: "evaluate", args });
    };
    node.waitForLoadState = async () => undefined;
    node.waitForTimeout = async () => undefined;
    node.keyboard = {
      press: async (key: string) => void this.calls.push({ path: "keyboard", method: "press", args: [key] }),
      type: async (text: string) => void this.calls.push({ path: "keyboard", method: "type", args: [text] }),
      up: async () => undefined,
    };
    return node as unknown as Page;
  }

  /** Every action as `method <locator path>`, for readable assertions. */
  trace(): string[] {
    return this.calls.map((call) => `${call.method} ${call.path}`);
  }

  clicks(): string[] {
    return this.calls.filter((call) => call.method === "click").map((call) => call.path);
  }

  pressed(): string[] {
    return this.calls.filter((call) => call.path === "keyboard").map((call) => call.args[0] as string);
  }

  gotos(): string[] {
    return this.calls.filter((call) => call.path === "page.goto").map((call) => call.args[0] as string);
  }

  routeHandlers(): Array<{
    matches: (url: URL) => boolean;
    handler: (route: { request(): { method(): string }; continue(): Promise<void> }) => Promise<void>;
  }> {
    return this.routed().map((entry) => ({
      ...entry,
      handler: entry.handler as (route: {
        request(): { method(): string };
        continue(): Promise<void>;
      }) => Promise<void>,
    }));
  }

  routed(): Array<{ matches: (url: URL) => boolean; handler: (route: unknown) => Promise<void> }> {
    return this.calls
      .filter((call) => call.path === "page.route")
      .map((call) => ({
        matches: call.args[0] as (url: URL) => boolean,
        handler: call.args[1] as (route: unknown) => Promise<void>,
      }));
  }

  requestListeners(): Array<(request: { url(): string; method(): string }) => void> {
    return this.calls
      .filter((call) => call.path === "page.on")
      .map((call) => call.args[1] as (request: { url(): string; method(): string }) => void);
  }

  unrouted(): number {
    return this.calls.filter((call) => call.path === "page.unroute").length;
  }
}

/** Assert that the trace contains each fragment, in order. */
function expectTrace(actual: string[], expected: string[]): void {
  const joined = actual.join("\n");
  let from = 0;
  for (const fragment of expected) {
    const at = joined.indexOf(fragment, from);
    expect(at, `missing ${fragment} in:\n${joined}`).toBeGreaterThanOrEqual(0);
    from = at + fragment.length;
  }
}

// Fragments of the driver's selectors, in the order they appear.
const BLOCK = "[data-draft-block]";
const NAME_BLOCK = `${BLOCK}").filter(`;
const HEADING = 'getByRole("heading").first()';
const SUBMIT = 'form button[type=\\"submit\\"]';
const TITLE = 'getByPlaceholder("Title").first()';
const CHIP = 'locator("form").getByRole("button").first()';
const DIALOG = 'getByRole("dialog")';

describe("navigation and the list", () => {
  it("navigates to the workspace drafts route", async () => {
    const recorder = new Recorder();
    await new WebNewDriver(recorder.page()).openDraftsPage("acme");
    expect(recorder.calls).toEqual([{ path: "page.goto", method: "goto", args: ["/acme/drafts/"] }]);
  });

  it("reads every draft name paragraph, trimming blanks", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "allTextContents" ? [" One ", "", "Two"] : undefined);
    await expect(new WebNewDriver(recorder.page()).visibleDraftNames()).resolves.toEqual(["One", "Two"]);
    expectTrace(recorder.trace(), ['allTextContents page.locator("p[data-draft-name]")']);
  });

  it("answers an empty name list when the read throws", async () => {
    const recorder = new Recorder();
    recorder.respond = () => {
      throw new Error("navigating");
    };
    await expect(new WebNewDriver(recorder.page()).visibleDraftNames()).resolves.toEqual([]);
  });

  it("counts blocks and catches a page mid-navigation", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "count" ? 3 : undefined);
    await expect(new WebNewDriver(recorder.page()).draftBlockCount()).resolves.toBe(3);
    expectTrace(recorder.trace(), [`count page.locator("[data-draft-block]")`]);
    const broken = new Recorder();
    broken.respond = () => {
      throw new Error("navigating");
    };
    await expect(new WebNewDriver(broken.page()).draftBlockCount()).resolves.toBe(0);
  });

  it("reads a block's text through the name-scoped block", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "textContent" ? " PAR One Todo " : undefined);
    await expect(new WebNewDriver(recorder.page()).draftBlockText("One")).resolves.toBe("PAR One Todo");
    expectTrace(recorder.trace(), [`textContent page.locator("[data-draft-block]")`, `filter`, `first`]);
  });

  it("settles on the empty state", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method, path }) => (method === "count" && path.includes("Half-written work items") ? 1 : 0);
    await expect(new WebNewDriver(recorder.page()).settleDraftsPage("acme")).resolves.toBe("empty");
    expectTrace(recorder.trace(), ['count page.getByText("Half-written work items")']);
  });

  it("settles on the zero-project state", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method, path }) => (method === "count" && path.includes('"No project"') ? 1 : 0);
    await expect(new WebNewDriver(recorder.page()).settleDraftsPage("acme")).resolves.toBe("empty");
  });

  it("settles on the list", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method, path }) => (method === "count" && path.includes(BLOCK) ? 2 : 0);
    await expect(new WebNewDriver(recorder.page()).settleDraftsPage("acme")).resolves.toBe("list");
    expect(recorder.trace().filter((line) => line.includes(BLOCK))).toHaveLength(1);
  });

  it("re-enters the route when the list stays blank", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method, path }) =>
      method === "count" && path.includes(BLOCK) && recorder.gotos().length > 1 ? 1 : 0;
    const realNow = Date.now;
    let clock = realNow();
    Date.now = () => (clock += 31_000);
    try {
      await expect(new WebNewDriver(recorder.page()).settleDraftsPage("acme")).resolves.toBe("list");
    } finally {
      Date.now = realNow;
    }
    expect(recorder.gotos()).toEqual(["/acme/drafts/", "/acme/drafts/"]);
  });
});

describe("create, copy and edit dialogs", () => {
  it("opens the draft modal from the empty-state action", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method, path }) => (method === "count" && path.includes("Create draft work item") ? 1 : 0);
    await new WebNewDriver(recorder.page()).openCreateDraftModal();
    expectTrace(recorder.clicks(), ["Create draft work item"]);
    expect(recorder.trace().at(-1)).toContain(`waitFor`);
    expect(recorder.trace().at(-1)).toContain(DIALOG);
    expect(recorder.trace().at(-1)).toContain(HEADING);
  });

  it("falls back to the header action once drafts exist", async () => {
    const recorder = new Recorder();
    await new WebNewDriver(recorder.page()).openCreateDraftModal();
    expect(recorder.clicks()).toHaveLength(1);
    expect(recorder.clicks()[0]).toContain("Draft a work item");
    expect(recorder.trace().at(-1)).toContain(`waitFor`);
    expect(recorder.trace().at(-1)).toContain(HEADING);
  });

  it("opens edit by double-clicking the named block, then waits for its heading", async () => {
    const recorder = new Recorder();
    await new WebNewDriver(recorder.page()).openDraftForEdit("One");
    const [wait, dblclick, heading] = recorder.trace();
    expect(wait).toContain("waitFor");
    expect(wait).toContain(NAME_BLOCK);
    expect(dblclick).toContain("dblclick");
    expect(dblclick).toContain(NAME_BLOCK);
    expect(heading).toContain("waitFor");
    expect(heading).toContain(HEADING);
    expect(recorder.trace()).toHaveLength(3);
  });

  it("submits the form and discards by name", async () => {
    const recorder = new Recorder();
    const driver = new WebNewDriver(recorder.page());
    await driver.submitCreateModal();
    await driver.clickModalDiscard();
    expectTrace(recorder.trace(), ["click", SUBMIT, "click", 'getByRole("button"']);
    // clickModalDiscard resolves the button by its accessible name.
    expect(recorder.trace()[1]).toContain("Discard");
  });

  it("reads the primary button label", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "textContent" ? " Save to Drafts " : undefined);
    await expect(new WebNewDriver(recorder.page()).modalPrimaryButtonLabel()).resolves.toBe("Save to Drafts");
    expectTrace(recorder.trace(), ["textContent", SUBMIT]);
  });

  it("reports dialog text and open state", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "count" ? 1 : undefined);
    const driver = new WebNewDriver(recorder.page());
    await expect(driver.modalTextContains("Add to project")).resolves.toBe(true);
    await expect(driver.createModalOpen()).resolves.toBe(true);
    expectTrace(recorder.trace(), ['count page.getByRole("dialog")', 'getByText("Add to project"', "count", DIALOG]);
  });

  it("reads the dialog heading, or blank when it is closed", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "textContent" ? " Create a draft " : undefined);
    await expect(new WebNewDriver(recorder.page()).createModalHeading()).resolves.toBe("Create a draft");
    const closed = new Recorder();
    await expect(new WebNewDriver(closed.page()).createModalHeading()).resolves.toBe("");
    expectTrace(closed.trace(), ["count", 'locator("h2").first()']);
  });

  it("types the title and refuses a value that did not stick", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "inputValue" ? "Draft name" : undefined);
    await new WebNewDriver(recorder.page()).fillCreateTitle("Draft name");
    expectTrace(recorder.trace(), [
      "waitFor",
      TITLE,
      "click",
      TITLE,
      "press keyboard",
      "press keyboard",
      "type keyboard",
      "inputValue",
      TITLE,
    ]);
    expect(recorder.pressed()).toEqual(["ControlOrMeta+a", "Backspace", "Draft name"]);
    const lost = new Recorder();
    await expect(new WebNewDriver(lost.page()).fillCreateTitle("Draft name")).rejects.toThrow("[parity]");
  });

  it("reads the title, then saves or publishes from the modal", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "inputValue" ? "Draft name" : undefined);
    const driver = new WebNewDriver(recorder.page());
    await expect(driver.createTitleValue()).resolves.toBe("Draft name");
    await driver.confirmSaveDraft();
    await driver.publishDraft();
    expectTrace(recorder.trace().slice(-2), ["click", "Save to Drafts", "click", "Publish issue"]);
  });
});

describe("block quick actions", () => {
  it("copies, moves and deletes through the named block's menu", async () => {
    const recorder = new Recorder();
    const driver = new WebNewDriver(recorder.page());
    await driver.copyDraftByName("One");
    await driver.moveDraftToProject("One");
    await driver.confirmMoveToProject();
    await driver.deleteDraftByName("One");
    expectTrace(recorder.trace(), [
      "waitFor",
      NAME_BLOCK,
      "click",
      "Actions for One",
      "menuitem",
      "Make a copy",
      "waitFor",
      NAME_BLOCK,
      "click",
      "Actions for One",
      "menuitem",
      "Move to project",
      "click",
      "Add to project",
      "waitFor",
      NAME_BLOCK,
      "click",
      "Actions for One",
      "menuitem",
      "Delete",
      'getByRole("button"',
      "Delete",
    ]);
  });
});

describe("the modal project chip", () => {
  it("reads the chip's text", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "textContent" ? " Atlas " : undefined);
    await expect(new WebNewDriver(recorder.page()).modalProjectName()).resolves.toBe("Atlas");
    expectTrace(recorder.trace(), ["waitFor", CHIP, "textContent", CHIP]);
  });

  it("picks an option and confirms the chip flipped", async () => {
    const recorder = new Recorder();
    let picked = false;
    recorder.respond = ({ method, path }) => {
      if (method === "click" && path.includes('getByRole("option"') && path.includes("Atlas")) {
        picked = true;
      }
      if (method === "textContent") return picked ? "Atlas" : "Current";
      return undefined;
    };
    await new WebNewDriver(recorder.page()).selectModalProject("Atlas");
    expectTrace(recorder.trace(), [
      "click",
      CHIP,
      "waitFor",
      'getByRole("option"',
      "Atlas",
      "click",
      'getByRole("option"',
      "Atlas",
      "waitFor",
      CHIP,
      "textContent",
      CHIP,
    ]);
  });
});

describe("sign-in retry", () => {
  it("delegates to the standard sign-in flow", async () => {
    const recorder = new Recorder();
    const driver = new WebNewDriver(recorder.page());
    const seen: string[] = [];
    driver.signInWithPassword = async (email: string, password: string) => {
      seen.push(`${email}:${password}`);
    };
    await driver.signInWithPasswordRetry("ada@example.com", "secret");
    expect(seen).toEqual(["ada@example.com:secret"]);
  });
});

// Fragments of the drafts region selectors, in the order they appear.
const REGION = 'getByRole("region",{"name":"Drafts"})';
const HEADER = `${REGION}.locator(":scope > *").first()`;
const COUNT_CHIP = `${HEADER}.getByTestId("drafts-count")`;
const LOAD_MORE = 'getByRole("button",{"name":"/Load more/"})';

describe("drafts list-surface reads (NEWFRONT-782)", () => {
  it("reads the tab title", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "title" ? "Workspace Draft" : undefined);
    await expect(new WebNewDriver(recorder.page()).draftsPageTitle()).resolves.toBe("Workspace Draft");
    expectTrace(recorder.trace(), ["title page.title"]);
  });

  it("reads the header block and collapses its whitespace", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "count" ? 1 : " Drafts\n  51 ");
    await expect(new WebNewDriver(recorder.page()).draftsHeaderText()).resolves.toBe("Drafts 51");
    expectTrace(recorder.trace(), ["count", HEADER, "textContent", HEADER]);
  });

  it("answers blank header and region text when the screen never mounts", async () => {
    const recorder = new Recorder();
    const driver = new WebNewDriver(recorder.page());
    await expect(driver.draftsHeaderText()).resolves.toBe("");
    await expect(driver.draftsMainText()).resolves.toBe("");
  });

  it("reads the region text, never the shell chrome", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "count" ? 1 : " Drafts\n Half-written work items ");
    await expect(new WebNewDriver(recorder.page()).draftsMainText()).resolves.toBe("Drafts Half-written work items");
    expectTrace(recorder.trace(), ["textContent", REGION]);
  });

  it("reads the count chip and answers null when it is absent", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method, path }) => {
      if (method === "count") return path.includes("drafts-count") ? 1 : 0;
      if (method === "textContent") return " 51 ";
      return undefined;
    };
    await expect(new WebNewDriver(recorder.page()).draftsCountChip()).resolves.toBe("51");
    expectTrace(recorder.trace(), ["textContent", COUNT_CHIP]);
    const empty = new Recorder();
    await expect(new WebNewDriver(empty.page()).draftsCountChip()).resolves.toBeNull();
  });

  it("reads row names in render order", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "allTextContents" ? [" Two ", "", "One"] : undefined);
    await expect(new WebNewDriver(recorder.page()).draftRowNames()).resolves.toEqual(["Two", "One"]);
    expectTrace(recorder.trace(), ['allTextContents page.locator("p[data-draft-name]")']);
  });

  it.each([
    ["draftsHeaderCreateState", "Draft a work item"],
    ["draftsEmptyCreateState", "Create draft work item"],
    ["draftsNoProjectCreateState", "Start your first project"],
  ] as const)("reads the %s gate: absent, disabled, enabled", async (method, name) => {
    for (const [count, disabled, state] of [
      [0, false, "absent"],
      [1, true, "disabled"],
      [1, false, "enabled"],
    ] as const) {
      const recorder = new Recorder();
      recorder.respond = ({ method: action }) =>
        action === "count" ? count : action === "isDisabled" ? disabled : undefined;
      await expect(new WebNewDriver(recorder.page())[method]()).resolves.toBe(state);
    }
    const traced = new Recorder();
    traced.respond = ({ method: action }) => (action === "count" ? 1 : false);
    await new WebNewDriver(traced.page())[method]();
    expectTrace(traced.trace(), [`getByRole("button",{"name":"${name}","exact":true})`]);
  });

  it("counts checkboxes and draggable rows", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method, path }) => (method === "count" && !path.includes("checkbox") ? 1 : 0);
    const driver = new WebNewDriver(recorder.page());
    await expect(driver.draftsMainCheckboxCount()).resolves.toBe(0);
    expectTrace(recorder.trace(), [`getByRole("checkbox")`]);
    const missing = new Recorder();
    await expect(new WebNewDriver(missing.page()).draftsMainCheckboxCount()).resolves.toBe(0);
    await expect(new WebNewDriver(missing.page()).draftRowsDraggableCount()).resolves.toBe(0);
    expectTrace(missing.trace(), ['count page.locator("[data-draft-block][draggable=\\"true\\"]")']);
  });

  it("presses a key on the page", async () => {
    const recorder = new Recorder();
    await new WebNewDriver(recorder.page()).draftsPressKey("?");
    expect(recorder.pressed()).toEqual(["?"]);
  });

  it("finds the skeleton behind either loader", async () => {
    const initial = new Recorder();
    initial.respond = ({ method, path }) => (method === "isVisible" ? path.includes("drafts-loading") : false);
    await expect(new WebNewDriver(initial.page()).draftsSkeletonVisible()).resolves.toBe(true);
    const next = new Recorder();
    next.respond = ({ method, path }) => (method === "isVisible" ? path.includes("drafts-load-more-loading") : false);
    await expect(new WebNewDriver(next.page()).draftsSkeletonVisible()).resolves.toBe(true);
    const settled = new Recorder();
    await expect(new WebNewDriver(settled.page()).draftsSkeletonVisible()).resolves.toBe(false);
  });

  it("reads load-more visibility", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "count" ? 1 : undefined);
    await expect(new WebNewDriver(recorder.page()).draftsLoadMoreVisible()).resolves.toBe(true);
    expectTrace(recorder.trace(), [LOAD_MORE]);
    const exhausted = new Recorder();
    await expect(new WebNewDriver(exhausted.page()).draftsLoadMoreVisible()).resolves.toBe(false);
  });

  it("clicks load-more and waits for the appended rows", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method, path }) => {
      if (method === "count" && path.includes(BLOCK)) return recorder.clicks().length > 0 ? 2 : 1;
      return undefined;
    };
    await new WebNewDriver(recorder.page()).draftsLoadMore();
    expectTrace(recorder.clicks(), [LOAD_MORE]);
  });

  it("holds list reads for the skeleton probe and lets writes through", async () => {
    const recorder = new Recorder();
    await new WebNewDriver(recorder.page()).draftsDelayList(4000);
    const [route] = recorder.routeHandlers();
    expect(route).toBeDefined();
    expect(route!.matches(new URL("https://app.test/api/workspaces/acme/draft-issues/?per_page=50"))).toBe(true);
    expect(route!.matches(new URL("https://app.test/api/workspaces/acme/projects/details/"))).toBe(false);
    const continued: string[] = [];
    await route!.handler({
      request: () => ({ method: () => "POST" }),
      continue: async () => void continued.push("write"),
    });
    expect(continued).toEqual(["write"]);
    const held = new Recorder();
    await new WebNewDriver(held.page()).draftsDelayList(1);
    const [probe] = held.routeHandlers();
    const reads: string[] = [];
    await probe!.handler({
      request: () => ({ method: () => "GET" }),
      continue: async () => void reads.push("read"),
    });
    expect(reads).toEqual(["read"]);
  });

  it("counts list reads across a refocus cycle", async () => {
    const recorder = new Recorder();
    await expect(new WebNewDriver(recorder.page()).draftsListFetchCountOnRefocus()).resolves.toBe(0);
    const [route] = recorder.routeHandlers();
    expect(route).toBeDefined();
    expect(route!.matches(new URL("https://app.test/api/workspaces/acme/draft-issues/"))).toBe(true);
    expect(route!.matches(new URL("https://app.test/api/workspaces/acme/issues/"))).toBe(false);
    const continued: string[] = [];
    await route!.handler({
      request: () => ({ method: () => "GET" }),
      continue: async () => void continued.push("read"),
    });
    expect(continued).toEqual(["read"]);
    expectTrace(recorder.trace(), ["evaluate page.evaluate", "unroute page.unroute"]);
  });

  it("collects draft entry traffic and nothing else", async () => {
    const recorder = new Recorder();
    let fed = false;
    recorder.respond = ({ method, path }) => {
      if (method === "count" && path.includes(BLOCK)) {
        if (!fed) {
          fed = true;
          for (const listener of recorder.requestListeners()) {
            listener({
              url: () => "https://app.test/api/workspaces/acme/draft-issues/?per_page=50",
              method: () => "GET",
            });
            listener({ url: () => "https://app.test/api/workspaces/acme/projects/details/", method: () => "GET" });
            listener({ url: () => "http://[invalid/draft-issues", method: () => "GET" });
          }
        }
        return 1;
      }
      return 0;
    };
    const seen = await new WebNewDriver(recorder.page()).draftsEntryRequests("acme");
    expect(seen).toEqual(["GET /api/workspaces/acme/draft-issues/"]);
    expect(recorder.gotos()).toEqual(["/acme/drafts"]);
    expectTrace(recorder.trace(), ["off page.off"]);
  });
});

describe("row identity (NEWFRONT-783, DRAFT-007)", () => {
  it("reads the marker from the row's first button", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "innerText" ? "  PAR " : undefined);
    await expect(new WebNewDriver(recorder.page()).draftRowProjectMarker("One")).resolves.toBe("PAR");
    expectTrace(recorder.trace(), ["waitFor", NAME_BLOCK, "innerText", 'getByRole("button").first()']);
  });

  it("reports whether the work-type mark is on the row", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method, path }) => (method === "count" && path.includes("data-draft-type-mark") ? 1 : 0);
    await expect(new WebNewDriver(recorder.page()).draftRowTypeMarkPresent("One")).resolves.toBe(true);
    const bare = new Recorder();
    await expect(new WebNewDriver(bare.page()).draftRowTypeMarkPresent("One")).resolves.toBe(false);
    expectTrace(bare.trace(), ["waitFor", NAME_BLOCK, "count", "data-draft-type-mark"]);
  });
});

describe("inline property reads (NEWFRONT-783, DRAFT-012-016)", () => {
  it("reads each property slot's text through its hook", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method, path }) => {
      if (method !== "innerText") return undefined;
      if (path.includes("draft-property") && path.includes("state")) return "  Doing ";
      if (path.includes("draft-property") && path.includes("priority")) return "High";
      if (path.includes("draft-property") && path.includes("labels")) return "Mine, Theirs";
      if (path.includes("draft-property") && path.includes("assignees")) return "  You ";
      return "";
    };
    const driver = new WebNewDriver(recorder.page());
    await expect(driver.draftStateText("One")).resolves.toBe("Doing");
    await expect(driver.draftPriorityText("One")).resolves.toBe("High");
    await expect(driver.draftLabelsText("One")).resolves.toBe("Mine, Theirs");
    await expect(driver.draftAssigneesText("One")).resolves.toBe("You");
    expectTrace(recorder.trace(), [
      "waitFor",
      "data-draft-property=",
      "state",
      "waitFor",
      "data-draft-property=",
      "priority",
      "waitFor",
      "data-draft-property=",
      "labels",
      "waitFor",
      "data-draft-property=",
      "assignees",
    ]);
  });

  it("opens each picker through its slot's trigger", async () => {
    const recorder = new Recorder();
    const driver = new WebNewDriver(recorder.page());
    await driver.draftOpenStatePicker("One");
    await driver.draftOpenPriorityPicker("One");
    await driver.draftOpenLabelsPicker("One");
    await driver.draftOpenAssigneesPicker("One");
    expectTrace(recorder.clicks(), [
      "data-draft-property=",
      "state",
      "data-draft-property=",
      "priority",
      "data-draft-property=",
      "labels",
      "data-draft-property=",
      "assignees",
    ]);
  });

  it("reads both date triggers as one span of text", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method, path }) => {
      if (method !== "innerText") return undefined;
      if (path.includes("draft-start-date-trigger")) return "Oct 12, 2026";
      if (path.includes("draft-due-date-trigger")) return "Oct 18, 2026";
      return "";
    };
    await expect(new WebNewDriver(recorder.page()).draftDatesText("One")).resolves.toBe("Oct 12, 2026 Oct 18, 2026");
  });

  it("opens each date picker through its trigger", async () => {
    const recorder = new Recorder();
    const driver = new WebNewDriver(recorder.page());
    await driver.draftOpenStartDatePicker("One");
    await driver.draftOpenDueDatePicker("One");
    expectTrace(recorder.clicks(), ["draft-start-date-trigger", "draft-due-date-trigger"]);
  });

  it("reports whether a picker option reads selected", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method, path }) => (method === "getAttribute" && path.includes("High") ? "true" : "false");
    const driver = new WebNewDriver(recorder.page());
    await expect(driver.draftPickerOptionSelected("High")).resolves.toBe(true);
    await expect(driver.draftPickerOptionSelected("Low")).resolves.toBe(false);
    expectTrace(recorder.trace(), ["waitFor", 'getByRole("option"', "High", "getAttribute"]);
  });

  it("edits through the row menu's edit entry", async () => {
    const recorder = new Recorder();
    await new WebNewDriver(recorder.page()).editDraftByName("One");
    expectTrace(recorder.trace(), ["waitFor", NAME_BLOCK, "click", "Actions for One", "menuitem", "Edit"]);
  });
});

describe("shared picker reads (NEWFRONT-783)", () => {
  it("reads combobox option labels without their glyphs", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method, path }) => {
      if (method === "count" && path.includes("listbox")) return 1;
      if (method === "allInnerTexts") return [" Doing ", "", "Backlog"];
      return undefined;
    };
    await expect(new WebNewDriver(recorder.page()).pickerOptionTexts()).resolves.toEqual(["Doing", "Backlog"]);
    expectTrace(recorder.trace(), ["count", "listbox", "allInnerTexts", 'getByRole("option"']);
  });

  it("falls back to the label picker's option buttons", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method }) => (method === "allInnerTexts" ? [" Mine ", ""] : undefined);
    await expect(new WebNewDriver(recorder.page()).pickerOptionTexts()).resolves.toEqual(["Mine"]);
    expectTrace(recorder.trace(), ["count", "listbox", "allInnerTexts", "Project labels"]);
  });

  it("picks from the listbox when one is open", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method, path }) => (method === "count" && path.includes("listbox") ? 1 : 0);
    await new WebNewDriver(recorder.page()).pickerPick("Doing");
    expectTrace(recorder.trace(), ["count", "listbox", "click", 'getByRole("option"', "Doing"]);
  });

  it("picks a label from the label picker's buttons", async () => {
    const recorder = new Recorder();
    await new WebNewDriver(recorder.page()).pickerPick("Mine");
    expectTrace(recorder.trace(), ["count", "listbox", "click", "Project labels", "Mine"]);
  });

  it("dismisses a picker with Escape", async () => {
    const recorder = new Recorder();
    await new WebNewDriver(recorder.page()).pickerPressEscape();
    expect(recorder.pressed()).toEqual(["Escape"]);
  });
});

describe("calendar days (NEWFRONT-783, DRAFT-016)", () => {
  it("picks an in-month day by its number", async () => {
    const recorder = new Recorder();
    await new WebNewDriver(recorder.page()).datePickerPickDay(12);
    expectTrace(recorder.trace(), ["click", "button[data-in-month"]);
  });

  it("reports whether an in-month day is disabled", async () => {
    const recorder = new Recorder();
    recorder.respond = ({ method, path }) =>
      method === "isDisabled" && path.includes("button[data-in-month") ? true : false;
    await expect(new WebNewDriver(recorder.page()).datePickerDayDisabled(21)).resolves.toBe(true);
    const open = new Recorder();
    await expect(new WebNewDriver(open.page()).datePickerDayDisabled(9)).resolves.toBe(false);
  });
});

describe("failing the next drafts PATCH (NEWFRONT-783, DRAFT-012)", () => {
  it("routes only drafts API traffic", async () => {
    const recorder = new Recorder();
    await new WebNewDriver(recorder.page()).draftFailNextPatch(500, 0);
    expect(recorder.routed()).toHaveLength(1);
    const { matches } = recorder.routed()[0]!;
    expect(matches(new URL("https://api.test/api/workspaces/acme/draft-issues/"))).toBe(true);
    expect(matches(new URL("https://api.test/api/workspaces/acme/draft-issues/123/"))).toBe(true);
    expect(matches(new URL("https://api.test/api/workspaces/acme/issues/"))).toBe(false);
    expect(matches(new URL("https://api.test/api/other/"))).toBe(false);
  });

  it("lets reads and other writes through untouched", async () => {
    const recorder = new Recorder();
    await new WebNewDriver(recorder.page()).draftFailNextPatch(500, 0);
    const { handler } = recorder.routed()[0]!;
    let continued = 0;
    let fulfilled = 0;
    const route = {
      request: () => ({ method: () => "GET" }),
      continue: async () => {
        continued += 1;
      },
      fulfill: async () => {
        fulfilled += 1;
      },
    };
    await handler(route);
    expect(continued).toBe(1);
    expect(fulfilled).toBe(0);
    expect(recorder.unrouted()).toBe(0);
  });

  it("fails the next PATCH once, then removes itself", async () => {
    const recorder = new Recorder();
    await new WebNewDriver(recorder.page()).draftFailNextPatch(500, 0);
    const { handler } = recorder.routed()[0]!;
    const fulfilled: unknown[] = [];
    const route = {
      request: () => ({ method: () => "PATCH" }),
      continue: async () => {},
      fulfill: async (answer: unknown) => {
        fulfilled.push(answer);
      },
    };
    await handler(route);
    expect(fulfilled).toEqual([{ status: 500, contentType: "application/json", body: "{}" }]);
    expect(recorder.unrouted()).toBe(1);
  });

  it("clears an armed failure, and stays quiet with none armed", async () => {
    const recorder = new Recorder();
    const driver = new WebNewDriver(recorder.page());
    await driver.draftClearPatchFailure();
    expect(recorder.unrouted()).toBe(0);
    await driver.draftFailNextPatch(500, 0);
    await driver.draftClearPatchFailure();
    expect(recorder.unrouted()).toBe(1);
    await driver.draftClearPatchFailure();
    expect(recorder.unrouted()).toBe(1);
  });
});
