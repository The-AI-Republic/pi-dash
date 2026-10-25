// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver reads the NEWFRONT-907 scenarios lean on
// (ISS-238–242): the sub-work-items Load more control, the waiting-for-runner
// banner, the sidebar property pickers and the in-place save error. A stub
// Page stands in for Playwright; each assertion covers the selector, the
// settle wait and the value the driver hands back.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new.js";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  isDisabled?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  focus?: (options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<void>;
  getAttribute?: (name: string) => Promise<string | null>;
  innerText?: () => Promise<string>;
  textContent?: () => Promise<string>;
  allTextContents?: () => Promise<string[]>;
  allInnerTexts?: () => Promise<string[]>;
  evaluate?: (fn: (element: never, arg?: never) => unknown, arg?: unknown) => Promise<unknown>;
  fill?: (value: string) => Promise<void>;
  inputValue?: () => Promise<string>;
  pressSequentially?: (value: string, options?: unknown) => Promise<void>;
  first?: () => StubLocator;
  last?: () => StubLocator;
  nth?: (index: number) => StubLocator;
  filter?: (options: unknown) => StubLocator;
  getByRole?: (role: string, options?: unknown) => StubLocator;
  locator?: (selector: string) => StubLocator;
  or?: (other: StubLocator) => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 1,
    isVisible: async () => true,
    isDisabled: async () => false,
    click: async () => undefined,
    focus: async () => undefined,
    waitFor: async () => undefined,
    getAttribute: async () => null,
    innerText: async () => "",
    textContent: async () => "",
    allTextContents: async () => [],
    allInnerTexts: async () => [],
    evaluate: async () => null,
    fill: async () => undefined,
    inputValue: async () => "",
    pressSequentially: async () => undefined,
    ...overrides,
  };
  const defaults: StubLocator = {
    first: () => node,
    last: () => node,
    nth: () => node,
    filter: () => node,
    getByRole: () => node,
    getByTestId: () => node,
    getByLabel: () => node,
    locator: () => node,
    or: () => node,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return node as unknown as Locator;
}

function pageOf(overrides: Partial<Record<string, unknown>> = {}): Page {
  return {
    getByTestId: () => locator(),
    getByRole: () => locator(),
    getByPlaceholder: () => locator(),
    getByText: () => locator(),
    locator: () => locator(),
    keyboard: { press: async () => undefined },
    waitForTimeout: async () => undefined,
    waitForFunction: async () => undefined,
    evaluate: async () => null,
    ...overrides,
  } as unknown as Page;
}

/** A page whose named locator answers come from one selector table. */
function pageWithSelectors(table: Record<string, () => StubLocator>, extra: Partial<Record<string, unknown>> = {}) {
  return pageOf({
    locator: (selector: string) => (table[selector] ?? (() => locator()))(),
    getByRole: (role: string, options?: unknown) => {
      const name = (options as { name?: string } | undefined)?.name ?? "";
      return (table[`role:${role}:${name}`] ?? (() => locator()))();
    },
    ...extra,
  });
}

describe("sub-work-items Load more (ISS-238)", () => {
  it("reports the control present when it is shown", async () => {
    const page = pageOf({
      locator: () => locator({ count: async () => 1, isVisible: async () => true }),
    });
    await expect(new WebNewDriver(page).widgetLoadMorePresent("Sub-work items")).resolves.toBe(true);
  });

  it("reports it absent when the section has no control", async () => {
    const page = pageOf({ locator: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(page).widgetLoadMorePresent("Sub-work items")).resolves.toBe(false);
  });

  it("reads a hidden control as absent", async () => {
    const page = pageOf({
      locator: () => locator({ count: async () => 1, isVisible: async () => false }),
    });
    await expect(new WebNewDriver(page).widgetLoadMorePresent("Sub-work items")).resolves.toBe(false);
  });

  it("clicks the control and settles once its label leaves the fetching state", async () => {
    let label = "Load more";
    const clicks: string[] = [];
    const control = locator({
      innerText: async () => label,
      click: async () => {
        clicks.push("load");
        label = "Loading more…";
        setTimeout(() => {
          label = "Load more";
        }, 0);
      },
    });
    const page = pageOf({
      locator: (selector: string) =>
        selector.includes("sub-issues-load-more") ? control : locator({ locator: () => control }),
    });
    await new WebNewDriver(page).widgetLoadMore("Sub-work items");
    expect(clicks).toEqual(["load"]);
    expect(label).toBe("Load more");
  });

  it("settles when the control's label cannot be read", async () => {
    // A control that detaches while the page lands: the blank read is not
    // the fetching label, so the helper returns.
    let reads = 0;
    const control = locator({
      count: async () => 1,
      innerText: async () => {
        reads += 1;
        if (reads === 1) throw new Error("element detached");
        return "Load more";
      },
    });
    const page = pageOf({
      locator: (selector: string) =>
        selector.includes("sub-issues-load-more") ? control : locator({ locator: () => control }),
    });
    await new WebNewDriver(page).widgetLoadMore("Sub-work items");
    expect(reads).toBe(1);
  });

  it("treats a control with no text as settled", async () => {
    const control = locator({ count: async () => 1, innerText: async () => null as unknown as string });
    const page = pageOf({
      locator: (selector: string) =>
        selector.includes("sub-issues-load-more") ? control : locator({ locator: () => control }),
    });
    await new WebNewDriver(page).widgetLoadMore("Sub-work items");
  });

  it("refuses to click a control that is not rendered", async () => {
    const page = pageOf({ locator: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(page).widgetLoadMore("Sub-work items")).rejects.toThrow("no load-more control");
  });
});

describe("sub-work-items Load more fetching state (ISS-238)", () => {
  it("reads a disabled control as mid-fetch", async () => {
    const control = locator({ count: async () => 1, isDisabled: async () => true });
    const page = pageOf({ locator: () => locator({ locator: () => control }) });
    await expect(new WebNewDriver(page).widgetLoadMoreFetching("Sub-work items")).resolves.toBe(true);
  });

  it("reads a live spinner inside the control as mid-fetch", async () => {
    const control = locator({
      count: async () => 1,
      isDisabled: async () => false,
      getByRole: (role: string) => locator({ count: async () => (role === "status" ? 1 : 0) }),
    });
    const page = pageOf({ locator: () => locator({ locator: () => control }) });
    await expect(new WebNewDriver(page).widgetLoadMoreFetching("Sub-work items")).resolves.toBe(true);
  });

  it("reads a settled control and a missing control as idle", async () => {
    const settled = locator({ count: async () => 1, getByRole: () => locator({ count: async () => 0 }) });
    const page = pageOf({ locator: () => locator({ locator: () => settled }) });
    await expect(new WebNewDriver(page).widgetLoadMoreFetching("Sub-work items")).resolves.toBe(false);
    const absent = pageOf({ locator: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(absent).widgetLoadMoreFetching("Sub-work items")).resolves.toBe(false);
  });
});

describe("nested parent Load more (ISS-238)", () => {
  /** A page whose sub-work-items panel resolves every selector to `row`. */
  function nestedPage(row: StubLocator): Page {
    const rows = locator({ filter: () => row, last: () => row, nth: () => row });
    const panel = locator({ locator: () => rows });
    return pageOf({ locator: () => panel });
  }

  /** The name matcher on a `getByRole` call, as a string. */
  function nameOf(options: unknown): string {
    const name = (options as { name?: unknown } | undefined)?.name;
    if (typeof name === "string") return name;
    if (name instanceof RegExp) return name.source;
    return "";
  }

  /** A row whose buttons answer `loadMore` and `toggle`. */
  function rowWith(loadMore: StubLocator, toggle: StubLocator, rest: Partial<StubLocator> = {}): StubLocator {
    return locator({
      count: async () => 1,
      getByRole: (_role: string, options?: unknown) => (nameOf(options).startsWith("^Load more$") ? loadMore : toggle),
      ...rest,
    });
  }

  it("reports a nested parent's control present when it is shown", async () => {
    const control = locator({ count: async () => 1, isVisible: async () => true, last: () => control });
    const page = nestedPage(rowWith(control, locator()));
    await expect(new WebNewDriver(page).widgetRowLoadMorePresent("Sub-work items", "Paged child 1000")).resolves.toBe(
      true
    );
  });

  it("reads a hidden nested control as absent", async () => {
    const control = locator({ count: async () => 1, isVisible: async () => false, last: () => control });
    const page = nestedPage(rowWith(control, locator()));
    await expect(new WebNewDriver(page).widgetRowLoadMorePresent("Sub-work items", "Paged child 1000")).resolves.toBe(
      false
    );
  });

  it("reports a nested parent with no control as absent", async () => {
    const page = nestedPage(rowWith(locator({ count: async () => 0 }), locator()));
    await expect(new WebNewDriver(page).widgetRowLoadMorePresent("Sub-work items", "Paged child 1000")).resolves.toBe(
      false
    );
  });

  it("clicks a nested control and settles once its label leaves the fetching state", async () => {
    let label = "Load more";
    const clicks: string[] = [];
    const control = locator({
      count: async () => 1,
      innerText: async () => label,
      click: async () => {
        clicks.push("load");
        label = "Loading more…";
        setTimeout(() => {
          label = "Load more";
        }, 0);
      },
      last: () => control,
    });
    await new WebNewDriver(nestedPage(rowWith(control, locator()))).widgetRowLoadMore(
      "Sub-work items",
      "Paged child 1000"
    );
    expect(clicks).toEqual(["load"]);
    expect(label).toBe("Load more");
  });

  it("refuses a nested row without its own control", async () => {
    const page = nestedPage(rowWith(locator({ count: async () => 0 }), locator()));
    await expect(new WebNewDriver(page).widgetRowLoadMore("Sub-work items", "Paged child 1000")).rejects.toThrow(
      "no nested load-more control"
    );
  });

  it("expands a nested row and settles once its children land", async () => {
    let expanded = false;
    const clicks: string[] = [];
    const toggle = locator({
      count: async () => 1,
      click: async () => {
        clicks.push("expand");
        expanded = true;
      },
    });
    const row = rowWith(locator({ count: async () => 0 }), toggle, {
      innerText: async () => (expanded ? "Paged child 1000\nPaged child 3000" : "Paged child 1000"),
      locator: (selector: string) =>
        selector === "[data-sub-issue-row]" ? locator({ count: async () => (expanded ? 1 : 0) }) : locator(),
    });
    await new WebNewDriver(nestedPage(row)).expandWidgetRowChildren("Sub-work items", "Paged child 1000");
    expect(clicks).toEqual(["expand"]);
  });

  it("settles an expansion that renders an empty note", async () => {
    const row = rowWith(locator({ count: async () => 0 }), locator({ count: async () => 1 }), {
      innerText: async () => "Paged child 1000\nNo children yet.",
      locator: () => locator({ count: async () => 0 }),
    });
    await new WebNewDriver(nestedPage(row)).expandWidgetRowChildren("Sub-work items", "Paged child 1000");
  });

  it("waits out the loading note before settling", async () => {
    let reads = 0;
    const row = rowWith(locator({ count: async () => 0 }), locator({ count: async () => 1 }), {
      innerText: async () => {
        reads += 1;
        return reads === 1 ? "Paged child 1000\nLoading children" : "Paged child 1000\nNo children yet.";
      },
      locator: () => locator({ count: async () => 0 }),
    });
    await new WebNewDriver(nestedPage(row)).expandWidgetRowChildren("Sub-work items", "Paged child 1000");
    expect(reads).toBeGreaterThan(1);
  });

  it("refuses a row that is not rendered", async () => {
    const page = nestedPage(locator({ count: async () => 0 }));
    await expect(new WebNewDriver(page).expandWidgetRowChildren("Sub-work items", "Paged child 1000")).rejects.toThrow(
      "no row"
    );
  });

  it("refuses a row with no expander", async () => {
    const page = nestedPage(rowWith(locator({ count: async () => 0 }), locator({ count: async () => 0 })));
    await expect(new WebNewDriver(page).expandWidgetRowChildren("Sub-work items", "Paged child 1000")).rejects.toThrow(
      "has no expander"
    );
  });
});

describe("waiting-for-runner banner (ISS-239/240)", () => {
  const bannerTable = (banner: StubLocator) => ({
    "role:region:Waiting for runner": () => banner,
  });

  it("reads presence from the labelled section", async () => {
    const banner = locator({ count: async () => 1, isVisible: async () => true });
    const page = pageWithSelectors(bannerTable(banner));
    await expect(new WebNewDriver(page).waitingBannerPresent()).resolves.toBe(true);
  });

  it("reads a missing section as no banner and no sentence", async () => {
    const page = pageOf({ getByRole: () => locator({ count: async () => 0 }) });
    const driver = new WebNewDriver(page);
    await expect(driver.waitingBannerPresent()).resolves.toBe(false);
    await expect(driver.waitingBannerText()).resolves.toBeNull();
  });

  it("reads the leading paragraph as the sentence, collapsing its whitespace", async () => {
    const banner = locator({
      locator: (selector: string) =>
        selector === ":scope > p"
          ? locator({ innerText: async () => "  Waiting for runner parity-a —\n busy, 2 runs ahead.  " })
          : locator(),
    });
    const page = pageWithSelectors(bannerTable(banner));
    await expect(new WebNewDriver(page).waitingBannerText()).resolves.toBe(
      "Waiting for runner parity-a — busy, 2 runs ahead."
    );
  });

  it("reads a blank sentence as null", async () => {
    const banner = locator({ locator: () => locator({ innerText: async () => "   " }) });
    const page = pageWithSelectors(bannerTable(banner));
    await expect(new WebNewDriver(page).waitingBannerText()).resolves.toBeNull();
  });

  it("reads a banner without its sentence paragraph as null", async () => {
    // A section that mounted ahead of its copy: the paragraph is absent
    // rather than blank, so the read must not throw on the missing node.
    const banner = locator({
      locator: (selector: string) => (selector === ":scope > p" ? locator({ count: async () => 0 }) : locator()),
    });
    const page = pageWithSelectors(bannerTable(banner));
    await expect(new WebNewDriver(page).waitingBannerText()).resolves.toBeNull();
  });

  it("reads a sentence whose text never arrived as null", async () => {
    const banner = locator({ locator: () => locator({ innerText: async () => null as unknown as string }) });
    const page = pageWithSelectors(bannerTable(banner));
    await expect(new WebNewDriver(page).waitingBannerText()).resolves.toBeNull();
  });

  it("lists the action row in render order, dropping blank labels", async () => {
    const banner = locator({
      locator: (selector: string) =>
        selector === ":scope > div > button"
          ? locator({
              allInnerTexts: async () => [" Run on any runner in PAR_pod_1 once ", " ", "Change runner", "Cancel run"],
            })
          : locator(),
    });
    const page = pageWithSelectors(bannerTable(banner));
    await expect(new WebNewDriver(page).waitingBannerActions()).resolves.toEqual([
      "Run on any runner in PAR_pod_1 once",
      "Change runner",
      "Cancel run",
    ]);
  });

  it("reads no actions without a banner", async () => {
    const page = pageOf({ getByRole: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(page).waitingBannerActions()).resolves.toEqual([]);
  });

  it("reads a named action's disabled flag and treats a missing one as inert", async () => {
    const banner = locator({
      getByRole: (role: string, options?: unknown) => {
        if (role !== "button") return locator();
        const name = (options as { name?: string }).name ?? "";
        if (name === "Cancel run") return locator({ count: async () => 1, isDisabled: async () => true });
        if (name === "Change runner") return locator({ count: async () => 1, isDisabled: async () => false });
        return locator({ count: async () => 0 });
      },
    });
    const driver = new WebNewDriver(pageWithSelectors(bannerTable(banner)));
    await expect(driver.waitingBannerActionDisabled("Cancel run")).resolves.toBe(true);
    await expect(driver.waitingBannerActionDisabled("Change runner")).resolves.toBe(false);
    await expect(driver.waitingBannerActionDisabled("Release")).resolves.toBe(true);
  });

  it("clicks the named action and refuses an absent one", async () => {
    const clicks: string[] = [];
    const banner = locator({
      getByRole: (role: string, options?: unknown) => {
        if (role !== "button") return locator();
        const name = (options as { name?: string }).name ?? "";
        if (name === "Cancel run") {
          return locator({ count: async () => 1, click: async () => void clicks.push(name) });
        }
        return locator({ count: async () => 0 });
      },
    });
    const driver = new WebNewDriver(pageWithSelectors(bannerTable(banner)));
    await driver.waitingBannerAction("Cancel run");
    expect(clicks).toEqual(["Cancel run"]);
    await expect(driver.waitingBannerAction("Release")).rejects.toThrow("no waiting banner action");
  });
});

describe("sidebar property rows (ISS-241/242)", () => {
  it("reads the row's value after its label", async () => {
    const row = locator({ innerText: async () => "Runs on  nf907-online · PAR_pod_1  " });
    const page = pageOf({ getByRole: () => row });
    await expect(new WebNewDriver(page).sidebarProperty("Runs on")).resolves.toBe("nf907-online · PAR_pod_1");
  });

  it("keeps a value that does not repeat the label", async () => {
    const row = locator({ innerText: async () => "Two weeks" });
    const page = pageOf({ getByRole: () => row });
    await expect(new WebNewDriver(page).sidebarProperty("Estimate")).resolves.toBe("Two weeks");
  });

  it("reads a value whose text content never arrived as null", async () => {
    // The row unmounted between the count and the read.
    const row = locator({ innerText: async () => null as unknown as string });
    const page = pageOf({ getByRole: () => row });
    await expect(new WebNewDriver(page).sidebarProperty("Runs on")).resolves.toBeNull();
  });

  it("reads an empty row and a missing row as null", async () => {
    const blank = pageOf({ getByRole: () => locator({ innerText: async () => "   " }) });
    await expect(new WebNewDriver(blank).sidebarProperty("Runs on")).resolves.toBeNull();
    const missing = pageOf({ getByRole: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(missing).sidebarProperty("Runs on")).resolves.toBeNull();
  });

  it("reports row presence and whether it carries a control", async () => {
    const withButton = locator({
      getByRole: () => locator({ count: async () => 1 }),
    });
    await expect(new WebNewDriver(pageOf({ getByRole: () => withButton })).sidebarRowPresent("Runs on")).resolves.toBe(
      true
    );
    await expect(
      new WebNewDriver(pageOf({ getByRole: () => withButton })).sidebarRowHasControl("Runs on")
    ).resolves.toBe(true);
    const textOnly = locator({ getByRole: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(pageOf({ getByRole: () => textOnly })).sidebarRowHasControl("Runs on")).resolves.toBe(
      false
    );
    const absent = locator({ count: async () => 0 });
    await expect(new WebNewDriver(pageOf({ getByRole: () => absent })).sidebarRowHasControl("Runs on")).resolves.toBe(
      false
    );
  });

  it("reads the trigger text and whether it is disabled", async () => {
    const trigger = locator({ innerText: async () => "  nf907-busy · PAR_pod_1 ", isDisabled: async () => true });
    const row = locator({ getByRole: () => trigger });
    const driver = new WebNewDriver(pageOf({ getByRole: () => row }));
    await expect(driver.propertyValueText("Runs on")).resolves.toBe("nf907-busy · PAR_pod_1");
    await expect(driver.propertyPickerDisabled("Runs on")).resolves.toBe(true);
    await expect(driver.propertyTriggerPresent("Runs on")).resolves.toBe(true);
  });

  it("opens the picker by click and by keyboard, then waits for the popup to settle", async () => {
    const events: string[] = [];
    const option = locator({ waitFor: async () => void events.push("settled") });
    const listbox = locator({ getByRole: () => locator({ first: () => option }) });
    const popup = locator({ getByRole: () => locator({ first: () => option }) });
    const trigger = locator({
      click: async () => void events.push("click"),
      focus: async () => void events.push("focus"),
    });
    const page = pageOf({
      getByRole: (role: string) => {
        if (role === "group") return locator({ getByRole: () => trigger });
        if (role === "listbox") return listbox;
        return popup;
      },
      keyboard: { press: async (key: string) => void events.push(key) },
    });
    const driver = new WebNewDriver(page);
    await driver.propertyOpenPicker("Runs on");
    expect(events).toEqual(["click", "settled"]);
    events.length = 0;
    await driver.propertyOpenPickerByKeyboard("Runs on");
    expect(events).toEqual(["focus", "Enter", "settled"]);
  });

  it("reads a trigger with no text as an empty value", async () => {
    const trigger = locator({ innerText: async () => null as unknown as string });
    const page = pageOf({ getByRole: () => locator({ getByRole: () => trigger }) });
    await expect(new WebNewDriver(page).propertyValueText("Runs on")).resolves.toBe("");
  });

  it("reads picker openness from the option list", async () => {
    const open = pageOf({
      getByRole: (role: string) => locator({ count: async () => (role === "listbox" ? 1 : 0) }),
    });
    await expect(new WebNewDriver(open).pickerOpen()).resolves.toBe(true);
    const closed = pageOf({ getByRole: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(closed).pickerOpen()).resolves.toBe(false);
  });

  it("picks an option from the listbox once it has mounted", async () => {
    const clicks: string[] = [];
    const option = locator({ click: async () => void clicks.push("option") });
    const listbox = locator({ count: async () => 1, getByRole: () => locator({ first: () => option }) });
    const page = pageOf({ getByRole: (role: string) => (role === "listbox" ? listbox : locator()) });
    await new WebNewDriver(page).pickerPick("nf907-online");
    expect(clicks).toEqual(["option"]);
  });

  it("retries after the popup mounts late, then falls back to the label list", async () => {
    // The kit mounts the popup a moment after the trigger click: the first
    // read misses it, the retry finds it. A picker with no listbox at all is
    // the label picker, whose options are buttons.
    const clicks: string[] = [];
    const listbox = locator({
      getByRole: () => locator({ first: () => locator({ click: async () => void clicks.push("late") }) }),
    });
    let reads = 0;
    const late = pageOf({
      getByRole: (role: string) => {
        if (role !== "listbox") return locator();
        reads += 1;
        return reads === 1 ? locator({ count: async () => 0 }) : listbox;
      },
    });
    await new WebNewDriver(late).pickerPick("nf907-online");
    expect(clicks).toEqual(["late"]);

    const label = pageOf({
      getByRole: () => locator({ count: async () => 0 }),
      locator: (selector: string) =>
        selector === 'ul[aria-label="Project labels"]'
          ? locator({
              getByRole: () => locator({ first: () => locator({ click: async () => void clicks.push("label") }) }),
            })
          : locator(),
    });
    await new WebNewDriver(label).pickerPick("bug");
    expect(clicks).toEqual(["late", "label"]);
  });
});

describe("picker options (ISS-241/242)", () => {
  function pickerPage(options: { parts: string[]; disabled?: boolean; optionCount?: number }) {
    const option = locator({
      count: async () => options.optionCount ?? 1,
      isDisabled: async () => options.disabled ?? false,
      waitFor: async () => undefined,
      locator: () => locator({ allTextContents: async () => options.parts }),
    });
    const listbox = locator({ getByRole: () => locator({ first: () => option }) });
    return pageOf({ getByRole: (role: string) => (role === "listbox" ? listbox : locator()) });
  }

  it("reads the trailing reason beside a runner's label", async () => {
    const page = pickerPage({
      parts: ["", "nf907-offline · offline · Parity Oracle", "Add the runner's owner as an assignee to choose it."],
    });
    await expect(new WebNewDriver(page).pickerOptionReason("nf907-offline")).resolves.toBe(
      "Add the runner's owner as an assignee to choose it."
    );
  });

  it("reads no reason from a bare row or an absent option", async () => {
    const bare = pickerPage({ parts: ["", "nf907-online · online · idle · Parity Oracle"] });
    await expect(new WebNewDriver(bare).pickerOptionReason("nf907-online")).resolves.toBeNull();
    const absent = pickerPage({ parts: [], optionCount: 0 });
    await expect(new WebNewDriver(absent).pickerOptionReason("nf907-online")).resolves.toBeNull();
  });

  it("reads a row with no content spans as carrying no reason", async () => {
    const page = pickerPage({ parts: [] });
    await expect(new WebNewDriver(page).pickerOptionReason("nf907-online")).resolves.toBeNull();
  });

  it("reads an option's disabled flag", async () => {
    const disabled = pickerPage({ parts: [], disabled: true });
    await expect(new WebNewDriver(disabled).pickerOptionDisabled("nf907-revoked")).resolves.toBe(true);
    const enabled = pickerPage({ parts: [], disabled: false });
    await expect(new WebNewDriver(enabled).pickerOptionDisabled("nf907-online")).resolves.toBe(false);
  });

  it("reads the search field's value, focus and whether it exists", async () => {
    const search = locator({
      inputValue: async () => "nf9",
      evaluate: async () => true,
      pressSequentially: async () => undefined,
      waitFor: async () => undefined,
    });
    const page = pageOf({ getByRole: () => search });
    const driver = new WebNewDriver(page);
    await expect(driver.pickerHasSearch()).resolves.toBe(true);
    await expect(driver.pickerSearchValue()).resolves.toBe("nf9");
    await expect(driver.pickerSearchFocused()).resolves.toBe(true);
    await driver.pickerSearch("nf9");
    const none = pageOf({ getByRole: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(none).pickerSearchFocused()).resolves.toBe(false);
  });

  it("reads a missing search field as not focused", async () => {
    // The label picker has no search input; the focus probe must not throw.
    const page = pageOf({ getByRole: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(page).pickerSearchFocused()).resolves.toBe(false);
  });

  it("reads focus from the search field's own active-element test", async () => {
    const page = pageOf({
      getByRole: () => locator({ count: async () => 1, evaluate: async () => true }),
    });
    await expect(new WebNewDriver(page).pickerSearchFocused()).resolves.toBe(true);
  });

  it("reads an unfocused search field", async () => {
    const page = pageOf({
      getByRole: () => locator({ count: async () => 1, evaluate: async () => false }),
    });
    await expect(new WebNewDriver(page).pickerSearchFocused()).resolves.toBe(false);
  });

  it("compares the search field against the document's active element", async () => {
    // The probe runs the browser-side comparison, so it is exercised with a
    // stub document rather than a canned answer.
    const field = { name: "search" };
    const focus = { activeElement: field };
    vi.stubGlobal("document", focus);
    const page = pageOf({
      getByRole: () => locator({ count: async () => 1, evaluate: async (fn: (el: unknown) => boolean) => fn(field) }),
    });
    try {
      await expect(new WebNewDriver(page).pickerSearchFocused()).resolves.toBe(true);
      focus.activeElement = { name: "other" };
      await expect(new WebNewDriver(page).pickerSearchFocused()).resolves.toBe(false);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("clicks the backdrop inside a dialog and the sidebar heading otherwise", async () => {
    // Inside a dialog the backdrop covers the page, so the press goes to
    // the empty corner; outside one it lands on the Properties heading.
    const pressed: string[] = [];
    const mouse = { click: async () => void pressed.push("corner") };
    const heading = locator({ click: async () => void pressed.push("heading") });
    const listbox = locator({ first: () => locator({ waitFor: async () => undefined }) });
    const inDialog = pageOf({
      getByRole: (role: string) => (role === "dialog" ? locator({ count: async () => 1 }) : listbox),
      mouse,
      getByText: () => locator({ first: () => heading }),
    });
    await new WebNewDriver(inDialog).pickerClickOutside();
    expect(pressed).toEqual(["corner"]);
    const onPage = pageOf({
      getByRole: (role: string) => (role === "dialog" ? locator({ count: async () => 0 }) : listbox),
      mouse,
      getByText: () => locator({ first: () => heading }),
    });
    await new WebNewDriver(onPage).pickerClickOutside();
    expect(pressed).toEqual(["corner", "heading"]);
  });

  it("reads the empty-picker line only when no option is offered", async () => {
    const listbox = locator({ getByRole: () => locator({ count: async () => 0 }) });
    const page = pageOf({
      getByRole: (role: string) =>
        role === "listbox" ? listbox : locator({ innerText: async () => "  No runners match.  " }),
    });
    await expect(new WebNewDriver(page).pickerEmptyText()).resolves.toBe("No runners match.");
    const filled = pageOf({ getByRole: () => locator({ getByRole: () => locator({ count: async () => 2 }) }) });
    await expect(new WebNewDriver(filled).pickerEmptyText()).resolves.toBe("");
  });

  it("reads no empty line when the popup carries no status node", async () => {
    const listbox = locator({ getByRole: () => locator({ count: async () => 0 }) });
    const page = pageOf({
      getByRole: (role: string) => (role === "listbox" ? listbox : locator({ count: async () => 0 })),
    });
    await expect(new WebNewDriver(page).pickerEmptyText()).resolves.toBe("");
  });

  it("reads a status node with no text as no empty line", async () => {
    const listbox = locator({ getByRole: () => locator({ count: async () => 0 }) });
    const page = pageOf({
      getByRole: (role: string) =>
        role === "listbox" ? listbox : locator({ innerText: async () => null as unknown as string }),
    });
    await expect(new WebNewDriver(page).pickerEmptyText()).resolves.toBe("");
  });
});

describe("in-place save error (ISS-242)", () => {
  it("reads the alert under the property row", async () => {
    const row = locator({
      getByRole: (role: string) =>
        role === "alert" ? locator({ innerText: async () => "  That runner is already dedicated.  " }) : locator(),
    });
    const page = pageOf({ getByRole: () => row });
    await expect(new WebNewDriver(page).propertyInlineError("Runs on")).resolves.toBe(
      "That runner is already dedicated."
    );
  });

  it("reads a missing or blank alert as null", async () => {
    const missing = pageOf({ getByRole: () => locator({ getByRole: () => locator({ count: async () => 0 }) }) });
    await expect(new WebNewDriver(missing).propertyInlineError("Runs on")).resolves.toBeNull();
    const blank = pageOf({
      getByRole: () => locator({ getByRole: () => locator({ innerText: async () => "  " }) }),
    });
    await expect(new WebNewDriver(blank).propertyInlineError("Runs on")).resolves.toBeNull();
  });

  it("reads an alert whose text never arrived as null", async () => {
    const page = pageOf({
      getByRole: () => locator({ getByRole: () => locator({ innerText: async () => null as unknown as string }) }),
    });
    await expect(new WebNewDriver(page).propertyInlineError("Runs on")).resolves.toBeNull();
  });
});

describe("property note (ISS-241)", () => {
  it("reads the advisory paragraph under the row and collapses its whitespace", async () => {
    const row = locator({
      locator: () =>
        locator({ count: async () => 1, innerText: async () => "  nf907-busy runs one issue   at a time. " }),
    });
    const page = pageOf({ getByRole: () => row });
    await expect(new WebNewDriver(page).propertyNote("Runs on")).resolves.toBe("nf907-busy runs one issue at a time.");
  });

  it("reads a missing row, a missing note and a blank note as null", async () => {
    await expect(
      new WebNewDriver(pageOf({ getByRole: () => locator({ count: async () => 0 }) })).propertyNote("Runs on")
    ).resolves.toBeNull();
    const noNote = pageOf({ getByRole: () => locator({ locator: () => locator({ count: async () => 0 }) }) });
    await expect(new WebNewDriver(noNote).propertyNote("Runs on")).resolves.toBeNull();
    const blank = pageOf({ getByRole: () => locator({ locator: () => locator({ innerText: async () => "  " }) }) });
    await expect(new WebNewDriver(blank).propertyNote("Runs on")).resolves.toBeNull();
    const unread = pageOf({
      getByRole: () => locator({ locator: () => locator({ innerText: async () => null as unknown as string }) }),
    });
    await expect(new WebNewDriver(unread).propertyNote("Runs on")).resolves.toBeNull();
  });
});

describe("signed-in probe", () => {
  it("reads a visible entry card as signed out and its absence as signed in", async () => {
    const card = pageOf({ getByPlaceholder: () => locator({ count: async () => 1, isVisible: async () => true }) });
    await expect(new WebNewDriver(card).signedIn()).resolves.toBe(false);
    const shell = pageOf({ getByPlaceholder: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(shell).signedIn()).resolves.toBe(true);
    const hidden = pageOf({ getByPlaceholder: () => locator({ count: async () => 1, isVisible: async () => false }) });
    await expect(new WebNewDriver(hidden).signedIn()).resolves.toBe(true);
  });
});

describe("widget progress and rows (ISS-174)", () => {
  it("reads the Done rollup from the sub-work-items header", async () => {
    const header = locator({ innerText: async () => "Sub-work items 3/120 Done" });
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="sub-issues-collapsible"]' ? locator({ first: () => header }) : locator(),
    });
    await expect(new WebNewDriver(page).widgetProgress("Sub-work items")).resolves.toBe("3/120 Done");
  });

  it("returns null for a missing header or one without a rollup", async () => {
    const missing = pageOf({ locator: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(missing).widgetProgress("Sub-work items")).resolves.toBeNull();
    const noRollup = pageOf({
      locator: () =>
        locator({ first: () => locator({ innerText: async () => "Sub-work items", count: async () => 1 }) }),
    });
    await expect(new WebNewDriver(noRollup).widgetProgress("Sub-work items")).resolves.toBeNull();
  });

  it("reads each sub-work-item row's text in order", async () => {
    const rows = locator({
      count: async () => 2,
      nth: (index: number) => locator({ innerText: async () => (index === 0 ? "NF-1" : "NF-2") }),
    });
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="sub-issues-section"]' ? locator({ locator: () => rows }) : locator(),
    });
    await expect(new WebNewDriver(page).widgetRowNames("Sub-work items")).resolves.toEqual(["NF-1", "NF-2"]);
  });

  it("gives up on a rollup header that cannot be read", async () => {
    // A header unmounted between the count and the read: the rollup reads
    // as absent rather than throwing out of a poll.
    const page = pageOf({
      locator: () =>
        locator({
          count: async () => 1,
          first: () =>
            locator({
              innerText: async () => {
                throw new Error("element detached");
              },
            }),
        }),
    });
    await expect(new WebNewDriver(page).widgetProgress("Sub-work items")).resolves.toBeNull();
  });

  it("reads another section's rollup through its shared header", async () => {
    const header = locator({ innerText: async () => "Relations 1/4 Done" });
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-widget-header="Relations"]' ? locator({ first: () => header }) : locator(),
    });
    await expect(new WebNewDriver(page).widgetProgress("Relations")).resolves.toBe("1/4 Done");
  });

  it("reads another section's rows from its own panel", async () => {
    const rows = locator({
      count: async () => 1,
      nth: () => locator({ innerText: async () => " design.md " }),
    });
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-widget-rows="Attachments"]' ? locator({ locator: () => rows }) : locator(),
    });
    await expect(new WebNewDriver(page).widgetRowNames("Attachments")).resolves.toEqual(["design.md"]);
  });

  it("toggles a shared-header section through that header", async () => {
    const clicks: string[] = [];
    const header = locator({
      count: async () => 1,
      getAttribute: async () => "false",
      click: async () => void clicks.push("toggle"),
    });
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-widget-header="Relations"]' ? locator({ first: () => header }) : locator(),
    });
    await new WebNewDriver(page).toggleWidgetSection("Relations");
    expect(clicks).toEqual(["toggle"]);
  });

  it("reads a header that cannot be read as having no rollup", async () => {
    const header = locator({ innerText: async () => null as unknown as string });
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="sub-issues-collapsible"]' ? locator({ first: () => header }) : locator(),
    });
    await expect(new WebNewDriver(page).widgetProgress("Sub-work items")).resolves.toBeNull();
  });

  it("reports no row when the sidebar never hydrates", async () => {
    const heading = locator({
      waitFor: async () => {
        throw new Error("timed out");
      },
    });
    const page = pageOf({
      getByText: () => locator({ first: () => heading }),
      getByRole: () => locator({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(page).propertyRowPresent("Runs on")).resolves.toBe(false);
  });
});
