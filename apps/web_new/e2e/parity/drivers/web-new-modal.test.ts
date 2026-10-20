// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver methods behind the create and
// row-confirmation scenarios (NEWFRONT-632, ISS-131–ISS-136). A stub Page
// stands in for Playwright; each case pins the selector, the scoping or
// the value a method reads or writes, not a browser run.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  click?: (options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<unknown>;
  fill?: (value: string, options?: unknown) => Promise<void>;
  press?: (key: string) => Promise<void>;
  focus?: () => Promise<void>;
  inputValue?: () => Promise<string>;
  allTextContents?: () => Promise<string[]>;
  getAttribute?: (name: string) => Promise<string | null>;
  filter?: (options: { hasText: string }) => StubLocator;
  evaluate?: (fn: (element: never) => unknown) => Promise<unknown>;
  first?: () => StubLocator;
  locator?: (selector: string) => StubLocator;
  getByRole?: (role: string, options?: unknown) => StubLocator;
  getByPlaceholder?: (placeholder: string) => StubLocator;
  getByText?: (text: string, options?: unknown) => StubLocator;
  getByTestId?: (id: string) => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 1,
    click: async () => undefined,
    waitFor: async () => undefined,
    fill: async () => undefined,
    press: async () => undefined,
    focus: async () => undefined,
    inputValue: async () => "",
    allTextContents: async () => [],
    getAttribute: async () => null,
    evaluate: async () => false,
    ...overrides,
  };
  const defaults: StubLocator = {
    first: () => node,
    filter: () => node,
    locator: () => node,
    getByRole: () => node,
    getByPlaceholder: () => node,
    getByText: () => node,
    getByTestId: () => node,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return node as unknown as Locator;
}

function pageOf(overrides: Partial<Record<string, unknown>> = {}): Page {
  return {
    getByRole: () => locator(),
    getByPlaceholder: () => locator(),
    getByText: () => locator(),
    getByTestId: () => locator(),
    locator: () => locator(),
    keyboard: { press: async () => undefined, type: async () => undefined },
    ...overrides,
  } as unknown as Page;
}

describe("create modal", () => {
  it("opens the modal from the header action and waits for the title", async () => {
    const opened = vi.fn(async () => undefined);
    const waited = vi.fn(async () => undefined);
    const page = pageOf({
      getByRole: (_role: string, options?: { name?: string }) => {
        if (options?.name === "Add work item") return locator({ click: opened });
        return locator();
      },
      getByPlaceholder: () => locator({ waitFor: waited }),
    });
    await new WebNewDriver(page).openCreateModal();
    expect(opened).toHaveBeenCalledTimes(1);
    expect(waited).toHaveBeenCalled();
  });

  it("reports the modal by the title field and reads its heading", async () => {
    const heading = locator({ textContent: async () => "  Create new work item  " });
    const page = pageOf({
      getByRole: () => locator({ filter: () => locator({ count: async () => 1, locator: () => heading }) }),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.createModalOpen()).resolves.toBe(true);
    await expect(driver.createModalHeading()).resolves.toBe("Create new work item");
  });

  it("reports a closed modal and an empty heading", async () => {
    const page = pageOf({
      getByRole: () => locator({ filter: () => locator({ count: async () => 0 }) }),
      locator: () => locator({ count: async () => 0 }),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.createModalOpen()).resolves.toBe(false);
    await expect(driver.createModalHeading()).resolves.toBe("");
  });

  it("types the title and proves the field holds it", async () => {
    const typed: string[] = [];
    const dialog = locator({
      getByPlaceholder: () => locator({ inputValue: async () => "Fresh report" }),
    });
    const page = pageOf({
      getByRole: () => locator({ filter: () => dialog }),
      keyboard: {
        press: async () => undefined,
        type: async (text: string) => void typed.push(text),
      },
    });
    await new WebNewDriver(page).fillCreateTitle("Fresh report");
    expect(typed).toEqual(["Fresh report"]);
  });

  it("fails loudly when the title field did not take the text", async () => {
    const dialog = locator({
      getByPlaceholder: () => locator({ inputValue: async () => "stale" }),
    });
    const page = pageOf({ getByRole: () => locator({ filter: () => dialog }) });
    await expect(new WebNewDriver(page).fillCreateTitle("Fresh report")).rejects.toThrow(/holds "stale"/);
  });

  it("reads focus, dialog text and placeholder presence", async () => {
    const page = pageOf({
      getByPlaceholder: (placeholder: string) =>
        locator({
          evaluate: async () => placeholder === "Title",
          count: async () => (placeholder === "Search for a project..." ? 1 : 0),
        }),
      locator: () => locator({ getByText: () => locator({ count: async () => 1 }) }),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.createTitleFocused()).resolves.toBe(true);
    await expect(driver.modalTextContains("restored later")).resolves.toBe(true);
    await expect(driver.modalHasPlaceholder("Search for a project...")).resolves.toBe(true);
  });

  it("focuses the title through the placeholder selector", async () => {
    const focused = vi.fn(async () => undefined);
    const page = pageOf({ getByPlaceholder: () => locator({ focus: focused }) });
    await new WebNewDriver(page).focusCreateTitle();
    expect(focused).toHaveBeenCalledTimes(1);
  });

  it("types into a dialog field by its placeholder", async () => {
    const typed: string[] = [];
    const page = pageOf({
      keyboard: { press: async () => undefined, type: async (text: string) => void typed.push(text) },
    });
    await new WebNewDriver(page).fillModalPlaceholder("Search for a project...", "Second Home");
    expect(typed).toEqual(["Second Home"]);
  });
});

describe("label picker", () => {
  it("opens the picker from the modal field", async () => {
    const opened = vi.fn(async () => undefined);
    const page = pageOf({
      locator: () =>
        locator({
          getByRole: (_role: string, options?: { name?: string }) =>
            locator({ click: options?.name === "Labels" ? opened : async () => undefined }),
        }),
    });
    await new WebNewDriver(page).openLabelsPicker();
    expect(opened).toHaveBeenCalledTimes(1);
  });

  it("creates a label through the search field and waits for the option", async () => {
    const filled: string[] = [];
    const pressed: string[] = [];
    const waited = vi.fn(async () => undefined);
    const page = pageOf({
      getByTestId: (id: string) =>
        id === "label-picker-search"
          ? locator({
              fill: async (value: string) => void filled.push(value),
              press: async (key: string) => void pressed.push(key),
            })
          : locator({ waitFor: waited }),
    });
    await new WebNewDriver(page).createLabelInModal("nf120-parity-label");
    expect(filled).toEqual(["nf120-parity-label"]);
    expect(pressed).toEqual(["Enter"]);
    expect(waited).toHaveBeenCalled();
  });

  it("reads the selection from the option's pressed state", async () => {
    const page = pageOf({
      getByTestId: () => locator({ getAttribute: async () => "true" }),
    });
    await expect(new WebNewDriver(page).selectedLabelVisible("nf120")).resolves.toBe(true);

    const missing = pageOf({ getByTestId: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(missing).selectedLabelVisible("nf120")).resolves.toBe(false);

    const unselected = pageOf({ getByTestId: () => locator({ getAttribute: async () => "false" }) });
    await expect(new WebNewDriver(unselected).selectedLabelVisible("nf120")).resolves.toBe(false);
  });
});

describe("row confirmations", () => {
  it("confirms delete inside the dialog", async () => {
    const clicked = vi.fn(async () => undefined);
    const page = pageOf({
      locator: () => locator({ getByRole: () => locator({ click: clicked }) }),
    });
    await new WebNewDriver(page).confirmDeleteIssue();
    expect(clicked).toHaveBeenCalledTimes(1);
  });

  it("counts exact page text", async () => {
    const page = pageOf({ getByText: () => locator({ count: async () => 2 }) });
    await expect(new WebNewDriver(page).countText("NF120 preview")).resolves.toBe(2);
  });
});

describe("hover preview card", () => {
  it("reads the open card's text, priority glyph and date tone", async () => {
    const page = pageOf({
      getByTestId: () =>
        locator({
          evaluate: async () => ({
            text: "WEB-42 Doing Ship the preview Apr 01, 2026",
            priorityIcon: "lucide-circle-alert",
            dateColor: "rgb(255, 0, 0)",
          }),
        }),
    });
    await expect(new WebNewDriver(page).hoverCardRead("Ship the preview")).resolves.toEqual({
      text: "WEB-42 Doing Ship the preview Apr 01, 2026",
      priorityIcon: "lucide-circle-alert",
      dateColor: "rgb(255, 0, 0)",
    });
  });

  it("reports an empty card when nothing is open", async () => {
    const page = pageOf({ getByTestId: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(page).hoverCardRead("Ship the preview")).resolves.toEqual({
      text: "",
      priorityIcon: "",
      dateColor: "",
    });
  });
});

describe("toast view action", () => {
  it("returns the address of the tab the action opened", async () => {
    const popup = {
      waitForLoadState: async () => undefined,
      url: () => "http://localhost:3000/acme/browse/PAR-7/",
    };
    const page = pageOf({
      waitForEvent: async () => popup,
      getByRole: () => locator(),
    });
    await expect(new WebNewDriver(page).openToastViewAction()).resolves.toBe(
      "http://localhost:3000/acme/browse/PAR-7/"
    );
  });
});
