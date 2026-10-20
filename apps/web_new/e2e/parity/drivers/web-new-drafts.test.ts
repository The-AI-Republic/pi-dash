// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver draft methods (NEWFRONT-643,
// ISS-137–141). A recording fake stands in for Playwright: each locator
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
  "click",
  "dblclick",
  "dispatchEvent",
  "waitFor",
  "textContent",
  "allTextContents",
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
        return JSON.stringify(record);
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
    if (method === "allTextContents") return [];
    if (method === "textContent" || method === "inputValue") return "";
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
