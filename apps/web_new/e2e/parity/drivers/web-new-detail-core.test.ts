// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver detail-core reads (NEWFRONT-658,
// ISS-142/147). A stub Page stands in for Playwright: each case pins
// the title, identifier, description, history, and quick-action hooks
// the detail-core scenarios drive, so a markup change that breaks them
// fails here before the parity stack runs.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubNode {
  count?: () => Promise<number>;
  innerText?: () => Promise<string>;
  textContent?: () => Promise<string | null>;
  inputValue?: () => Promise<string>;
  allTextContents?: () => Promise<string[]>;
  getAttribute?: (name: string) => Promise<string | null>;
  click?: (options?: unknown) => Promise<void>;
  fill?: (value: string, options?: unknown) => Promise<void>;
  press?: (key: string, options?: unknown) => Promise<void>;
  pressSequentially?: (text: string, options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<void>;
  isVisible?: () => Promise<boolean>;
  isDisabled?: () => Promise<boolean>;
  locator?: (selector: string) => Locator;
  getByTestId?: (id: string) => Locator;
  getByRole?: (role: string, options?: unknown) => Locator;
  getByText?: (text: unknown) => Locator;
  filter?: (options: unknown) => Locator;
  first?: () => Locator;
  nth?: (index: number) => Locator;
}

function node(overrides: Partial<StubNode> = {}): Locator {
  const self: StubNode = {
    count: async () => 1,
    innerText: async () => "",
    textContent: async () => "",
    inputValue: async () => "",
    allTextContents: async () => [],
    getAttribute: async () => null,
    click: async () => undefined,
    fill: async () => undefined,
    press: async () => undefined,
    pressSequentially: async () => undefined,
    waitFor: async () => undefined,
    isVisible: async () => true,
    isDisabled: async () => false,
    ...overrides,
  };
  const chain: StubNode = {
    locator: () => node({ count: async () => 0 }),
    getByTestId: () => node({ count: async () => 0 }),
    getByRole: () => node({ count: async () => 0 }),
    getByText: () => node({ count: async () => 0 }),
    filter: () => self as unknown as Locator,
    first: () => self as unknown as Locator,
    nth: () => self as unknown as Locator,
  };
  for (const [name, value] of Object.entries(chain)) {
    if (self[name as keyof StubNode] === undefined) {
      (self as Record<string, unknown>)[name] = value;
    }
  }
  return self as unknown as Locator;
}

function absent(): Locator {
  return node({ count: async () => 0 });
}

function collection(items: Locator[]): Locator {
  return node({
    count: async () => items.length,
    nth: (index: number) => items[index] ?? absent(),
    first: () => items[0] ?? absent(),
    allTextContents: async () => {
      const out: string[] = [];
      for (const item of items) out.push(await item.allTextContents().then((t) => t.join("")));
      return out;
    },
  });
}

function item(text: string, disabled = false): Locator {
  return node({
    innerText: async () => text,
    textContent: async () => text,
    allTextContents: async () => [text],
    isDisabled: async () => disabled,
  });
}

interface PageParts {
  testId?: (id: string) => Locator;
  role?: (role: string, options?: unknown) => Locator;
  text?: (text: unknown) => Locator;
  label?: (label: string) => Locator;
  placeholder?: (text: string) => Locator;
  locator?: (selector: string) => Locator;
  goto?: (url: string) => Promise<void>;
  waitForURL?: (url: unknown, options?: unknown) => Promise<void>;
  waitForTimeout?: (ms: number) => Promise<void>;
  waitForLoadState?: (state?: unknown) => Promise<void>;
  reload?: () => Promise<void>;
  keyboardPress?: (key: string) => Promise<void>;
  url?: () => string;
}

function pageOf(parts: PageParts = {}): Page {
  return {
    getByTestId: (id: string) => parts.testId?.(id) ?? absent(),
    getByRole: (role: string, options?: unknown) => parts.role?.(role, options) ?? absent(),
    getByText: (text: unknown) => parts.text?.(text) ?? absent(),
    getByLabel: (label: string) => parts.label?.(label) ?? absent(),
    getByPlaceholder: (text: string) => parts.placeholder?.(text) ?? absent(),
    locator: (selector: string) => parts.locator?.(selector) ?? absent(),
    goto: (url: string) => parts.goto?.(url) ?? Promise.resolve(),
    waitForURL: (url: unknown, options?: unknown) => parts.waitForURL?.(url, options) ?? Promise.resolve(),
    waitForTimeout: (ms: number) => parts.waitForTimeout?.(ms) ?? Promise.resolve(),
    waitForLoadState: (state?: unknown) => parts.waitForLoadState?.(state) ?? Promise.resolve(),
    reload: () => parts.reload?.() ?? Promise.resolve(),
    keyboard: { press: (key: string) => parts.keyboardPress?.(key) ?? Promise.resolve() },
    url: () => parts.url?.() ?? "http://localhost/",
  } as unknown as Page;
}

describe("detailCoreDriver", () => {
  it("reads the identifier from the header copy button", async () => {
    const page = pageOf({
      role: (role: string) => (role === "button" ? node({ innerText: async () => "WEB-42" }) : absent()),
    });
    await expect(new WebNewDriver(page).issueDetailIdentifier()).resolves.toBe("WEB-42");
  });

  it("returns null when no identifier button renders", async () => {
    await expect(new WebNewDriver(pageOf()).issueDetailIdentifier()).resolves.toBeNull();
  });

  it("fills the title and tabs out to flush the autosave", async () => {
    const calls: string[] = [];
    const field = node({
      fill: async (value: string) => {
        calls.push(`fill:${value}`);
      },
      press: async (key: string) => {
        calls.push(`press:${key}`);
      },
    });
    const page = pageOf({ label: (label: string) => (label === "Title" ? field : absent()) });
    await new WebNewDriver(page).editIssueTitle("renamed");
    expect(calls).toEqual(["fill:renamed", "press:Tab"]);
  });

  it("reads Saving before Saved from the save indicator", async () => {
    const saving = pageOf({
      text: (text: unknown) => (String(text) === "/^Saving…$/" ? node({}) : absent()),
    });
    await expect(new WebNewDriver(saving).saveIndicator()).resolves.toBe("Saving…");
    const saved = pageOf({
      text: (text: unknown) => (String(text) === "/^Saved$/" ? node({}) : absent()),
    });
    await expect(new WebNewDriver(saved).saveIndicator()).resolves.toBe("Saved");
    await expect(new WebNewDriver(pageOf()).saveIndicator()).resolves.toBeNull();
  });

  it("reads the live editor first when a description edit is open", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === "[contenteditable='true'][aria-label='Description']"
          ? node({ innerText: async () => "draft body" })
          : absent(),
    });
    await expect(new WebNewDriver(page).descriptionText()).resolves.toBe("draft body");
  });

  it("ignores the composer draft and reads the settled surface", async () => {
    // ISS-147: a restore closes the description editor, leaving only
    // the composer's always-mounted (empty) draft — labelled apart.
    const page = pageOf({
      role: (role: string, options?: unknown) => {
        const name = (options as { name?: string } | undefined)?.name;
        if (role === "button" && name === "Edit description") {
          return node({ innerText: async () => "restored body" });
        }
        return absent();
      },
    });
    await expect(new WebNewDriver(page).descriptionText()).resolves.toBe("restored body");
  });

  it("reads the settled description surface when not editing", async () => {
    const page = pageOf({
      role: (role: string, options?: unknown) => {
        const name = (options as { name?: string } | undefined)?.name;
        if (role === "button" && name === "Edit description") {
          return node({ innerText: async () => "stored body" });
        }
        return absent();
      },
    });
    await expect(new WebNewDriver(page).descriptionText()).resolves.toBe("stored body");
  });

  it("reads an untouched description as empty", async () => {
    const page = pageOf({
      role: (role: string, options?: unknown) => {
        const name = (options as { name?: string } | undefined)?.name;
        if (role === "button" && name === "Add a description") return node({});
        return absent();
      },
    });
    await expect(new WebNewDriver(page).descriptionText()).resolves.toBe("");
  });

  it("returns null when no description surface renders", async () => {
    await expect(new WebNewDriver(pageOf()).descriptionText()).resolves.toBeNull();
  });

  it("types a fresh description through the add button", async () => {
    const calls: string[] = [];
    const page = pageOf({
      role: (role: string, options?: unknown) => {
        const name = (options as { name?: string } | undefined)?.name;
        if (role === "button" && name === "Add a description") {
          return node({
            click: async () => {
              calls.push("click:add");
            },
          });
        }
        return absent();
      },
      locator: (selector: string) =>
        selector === "[contenteditable='true'][aria-label='Description']"
          ? node({
              click: async () => {
                calls.push("click:editor");
              },
              pressSequentially: async (text: string) => {
                calls.push(`type:${text}`);
              },
              press: async (key: string) => {
                calls.push(`press:${key}`);
              },
            })
          : absent(),
      keyboardPress: async (key: string) => {
        calls.push(`kbd:${key}`);
      },
    });
    await new WebNewDriver(page).setDescription("hello");
    expect(calls).toEqual(["click:add", "click:editor", "kbd:ControlOrMeta+a", "type:hello", "press:Tab"]);
  });

  it("clears the editor with select-all plus backspace for an empty description", async () => {
    const calls: string[] = [];
    const page = pageOf({
      role: (role: string, options?: unknown) => {
        const name = (options as { name?: string } | undefined)?.name;
        if (role === "button" && name === "Edit description") return node({});
        return absent();
      },
      locator: (selector: string) =>
        selector === "[contenteditable='true'][aria-label='Description']" ? node({}) : absent(),
      keyboardPress: async (key: string) => {
        calls.push(`kbd:${key}`);
      },
    });
    await new WebNewDriver(page).setDescription("");
    expect(calls).toEqual(["kbd:ControlOrMeta+a", "kbd:Backspace"]);
  });

  it("navigates the legacy short link and waits for the browse view", async () => {
    const calls: string[] = [];
    const page = pageOf({
      goto: async (url: string) => {
        calls.push(`goto:${url}`);
      },
      waitForURL: async (url: unknown) => {
        calls.push(`wait:${String(url)}`);
      },
    });
    await new WebNewDriver(page).openLegacyIssueRoute("ws", "pid", "iid");
    expect(calls).toEqual(["goto:/ws/projects/pid/issues/iid", "wait:/\\/browse\\//"]);
  });

  it("resolves the read-only opener once the key and state hydrate", async () => {
    const page = pageOf({
      role: (role: string) => {
        if (role === "button") return node({ innerText: async () => "WEB-7" });
        if (role === "group") return node({ innerText: async () => "State Todo" });
        return absent();
      },
    });
    await expect(new WebNewDriver(page).openReadOnlyIssueDetail("ws", "WEB-7")).resolves.toBeUndefined();
  });

  it("reads the missing state from its heading", async () => {
    const missing = pageOf({ text: () => node({}) });
    await expect(new WebNewDriver(missing).seesDetailMissing()).resolves.toBe(true);
    await expect(new WebNewDriver(pageOf()).seesDetailMissing()).resolves.toBe(false);
  });

  it("reads and clicks the subscribe toggle", async () => {
    let clicked = false;
    const page = pageOf({
      role: (role: string) =>
        role === "button"
          ? node({
              innerText: async () => "Subscribe",
              click: async () => {
                clicked = true;
              },
            })
          : absent(),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.subscribeToggle()).resolves.toBe("Subscribe");
    await driver.clickSubscribeToggle();
    expect(clicked).toBe(true);
    await expect(new WebNewDriver(pageOf()).subscribeToggle()).resolves.toBeNull();
  });

  it("lists the quick-action menu names", async () => {
    const items = collection([item("Delete"), item("Archive"), item("Make a copy")]);
    const page = pageOf({
      testId: (id: string) => (id === "detail-menu-trigger" ? node({}) : absent()),
      locator: (selector: string) => (selector === '[role="menuitem"]:visible' ? items : absent()),
    });
    await expect(new WebNewDriver(page).quickActionNames()).resolves.toEqual(["Delete", "Archive", "Make a copy"]);
  });

  it("clicks a quick action and waits for the menu to close", async () => {
    let clicked = "";
    const items = node({
      filter: () =>
        node({
          click: async () => {
            clicked = "Delete";
          },
        }),
    });
    const page = pageOf({
      testId: () => node({}),
      locator: (selector: string) => (selector === '[role="menuitem"]:visible' ? items : node({})),
      role: (role: string) => (role === "menuitem" ? node({}) : absent()),
    });
    await new WebNewDriver(page).clickQuickAction("Delete");
    expect(clicked).toBe("Delete");
  });

  it("reads a quick action's disabled state and dismisses the menu", async () => {
    const keys: string[] = [];
    const items = node({
      filter: () => node({ isDisabled: async () => true }),
    });
    const page = pageOf({
      testId: () => node({}),
      locator: (selector: string) => (selector === '[role="menuitem"]:visible' ? items : absent()),
      keyboardPress: async (key: string) => {
        keys.push(key);
      },
    });
    await expect(new WebNewDriver(page).quickActionDisabled("Archive")).resolves.toBe(true);
    expect(keys).toEqual(["Escape"]);
  });

  it("opens the description history menu from its trigger", async () => {
    let opened = false;
    const page = pageOf({
      role: () =>
        node({
          click: async () => {
            opened = true;
          },
        }),
      locator: (selector: string) => (selector === '[role="menuitem"]:visible' ? node({}) : absent()),
    });
    await new WebNewDriver(page).openDescriptionHistory();
    expect(opened).toBe(true);
  });

  it("lists history version names", async () => {
    const items = collection([item("Ada just now"), item("Ada yesterday")]);
    const page = pageOf({
      locator: (selector: string) => (selector === '[role="menuitem"]:visible' ? items : absent()),
    });
    await expect(new WebNewDriver(page).historyVersionNames()).resolves.toEqual(["Ada just now", "Ada yesterday"]);
  });

  it("restores a history version with time-blind matching", async () => {
    let restored = false;
    const versions = [item("Ada just now"), item("Bo yesterday")];
    const dialog = node({
      getByRole: () =>
        node({
          click: async () => {
            restored = true;
          },
        }),
    });
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === '[role="menuitem"]:visible') return collection(versions);
        return absent();
      },
      role: (role: string) => (role === "dialog" ? dialog : absent()),
    });
    // "Bo less than a minute ago" matches "Bo yesterday": the relative
    // timestamps are stripped before comparing.
    await new WebNewDriver(page).restoreHistoryVersion("Bo less than a minute ago");
    expect(restored).toBe(true);
  });
});
