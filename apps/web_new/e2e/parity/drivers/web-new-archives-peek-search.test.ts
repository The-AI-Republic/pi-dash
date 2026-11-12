// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the archived peek and search driver methods shared by
// the work-items, cycles and modules tabs (NEWFRONT-699, ARCH-005–007;
// reused by NEWFRONT-701/702/703). The same method must find the right
// row, peek panel and search box whatever tab the address shows.
import type { Locator, Page } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

function loc(
  overrides: {
    count?: () => Promise<number>;
    isVisible?: () => Promise<boolean>;
    waitFor?: (options?: unknown) => Promise<void>;
    click?: (options?: unknown) => Promise<void>;
    focus?: () => Promise<void>;
    scrollIntoViewIfNeeded?: () => Promise<void>;
    innerText?: () => Promise<string>;
    inputValue?: () => Promise<string>;
    fill?: (text: string, options?: unknown) => Promise<void>;
    locator?: (selector: string, options?: unknown) => Locator;
    getByRole?: (role: string, options?: unknown) => Locator;
    getByPlaceholder?: (text: string, options?: unknown) => Locator;
    getByTestId?: (id: string) => Locator;
  } = {}
): Locator {
  const node: Record<string, unknown> = {};
  node["count"] = overrides.count ?? (async () => 1);
  node["isVisible"] = overrides.isVisible ?? (async () => true);
  node["waitFor"] = overrides.waitFor ?? (async () => undefined);
  node["click"] = overrides.click ?? (async () => undefined);
  node["focus"] = overrides.focus ?? (async () => undefined);
  node["scrollIntoViewIfNeeded"] = overrides.scrollIntoViewIfNeeded ?? (async () => undefined);
  node["innerText"] = overrides.innerText ?? (async () => "");
  node["inputValue"] = overrides.inputValue ?? (async () => "");
  node["fill"] = overrides.fill ?? (async () => undefined);
  const self = node as unknown as Locator;
  node["first"] = () => self;
  node["last"] = () => self;
  node["nth"] = () => self;
  node["locator"] = (selector: string, options?: unknown) => overrides.locator?.(selector, options) ?? self;
  node["getByRole"] = (role: string, options?: unknown) => overrides.getByRole?.(role, options) ?? self;
  node["getByPlaceholder"] = (text: string, options?: unknown) => overrides.getByPlaceholder?.(text, options) ?? self;
  node["getByTestId"] = (id: string) => overrides.getByTestId?.(id) ?? self;
  return self;
}

interface StubConfig {
  url: string | (() => string);
  /** Archives content slot: resolves the inner selectors the methods read. */
  content?: (access: "locator" | "testid" | "role" | "placeholder", value: string, options?: unknown) => Locator;
  panel?: Locator;
  press?: (key: string) => void;
}

function stubPage(config: StubConfig): { page: Page; presses: string[] } {
  let now = 1_000_000;
  vi.spyOn(Date, "now").mockImplementation(() => now);
  const presses: string[] = [];
  const content = loc({
    locator: (selector, options) => config.content?.("locator", selector, options) ?? loc(),
    getByTestId: (id) => config.content?.("testid", id) ?? loc(),
    getByRole: (role, options) => config.content?.("role", role, options) ?? loc(),
    getByPlaceholder: (text, options) => config.content?.("placeholder", text, options) ?? loc(),
  });
  const page = {
    url: () => (typeof config.url === "function" ? config.url() : config.url),
    locator: (selector: string) =>
      selector === '[data-testid="archives-content"]'
        ? content
        : selector.includes("archived-module-preview")
          ? (config.panel ?? loc())
          : loc(),
    getByTestId: () => config.panel ?? loc(),
    // No row menu is open unless a case says so: the peek opener closes an
    // open menu before it presses the row.
    getByRole: () => loc({ count: async () => 0 }),
    getByText: () => loc(),
    waitForTimeout: async (ms: number) => {
      now += ms;
    },
    keyboard: {
      press: async (key: string) => {
        presses.push(key);
        config.press?.(key);
      },
    },
  };
  return { page: page as unknown as Page, presses };
}

afterEach(() => {
  vi.restoreAllMocks();
  document.body.innerHTML = "";
});

describe("archivesPeekOpenFirst across tabs", () => {
  it("opens an issue row from its title", async () => {
    let url = "http://new.test/acme/projects/p1/archives/issues";
    const { page } = stubPage({
      url: () => url,
      content: (access, value) =>
        access === "locator" && value === 'a[id^="issue-"]'
          ? loc({
              locator: (inner) =>
                inner === "p"
                  ? loc({
                      click: async () => {
                        url = "http://new.test/acme/projects/p1/archives/issues?peekIssueId=i1";
                      },
                    })
                  : loc(),
            })
          : loc(),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesPeekOpenFirst("Old login");

    expect(url).toContain("peekIssueId=i1");
  });

  it("opens a cycle peek from its quick-look control", async () => {
    let url = "http://new.test/acme/projects/p1/archives/cycles";
    let quickLookClicks = 0;
    const { page } = stubPage({
      url: () => url,
      content: (access, value) =>
        access === "locator" && value === '[data-testid="cycle-row"]'
          ? loc({
              getByRole: (role) =>
                role === "button"
                  ? loc({
                      click: async () => {
                        quickLookClicks += 1;
                        url = "http://new.test/acme/projects/p1/archives/cycles?peekCycle=c1";
                      },
                    })
                  : loc(),
            })
          : loc(),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesPeekOpenFirst("Archived alpha");

    expect(quickLookClicks).toBe(1);
    expect(url).toContain("peekCycle=c1");
  });

  it("opens a module peek from the row's labelled preview control", async () => {
    let url = "http://new.test/acme/projects/p1/archives/modules";
    const names: string[] = [];
    let previewClicks = 0;
    const { page } = stubPage({
      url: () => url,
      content: (access, value) => {
        if (access !== "locator" || value !== '[data-testid="modules-row"], [data-testid="modules-board-card"]') {
          return loc();
        }
        return loc({
          getByRole: (role, options) => {
            if (role !== "button") return loc({ count: async () => 0 });
            names.push(String((options as { name?: unknown }).name));
            return loc({
              click: async () => {
                previewClicks += 1;
                url = "http://new.test/acme/projects/p1/archives/modules?peekModule=m1";
              },
            });
          },
          // The name link navigates to the detail, so it must stay unpressed.
          locator: () =>
            loc({
              click: async () => {
                throw new Error("the module peek must not use the name link");
              },
            }),
        });
      },
    });
    const driver = new WebNewDriver(page);

    await driver.archivesPeekOpenFirst("Archived module");

    expect(previewClicks).toBe(1);
    expect(names).toEqual(["/^Open preview of Archived module$/"]);
    expect(url).toContain("peekModule=m1");
  });

  it("falls back to keyboard activation when the press opens nothing", async () => {
    let url = "http://new.test/acme/projects/p1/archives/cycles";
    const { page, presses } = stubPage({
      url: () => url,
      press: (key) => {
        if (key === "Enter") url = "http://new.test/acme/projects/p1/archives/cycles?peekCycle=c2";
      },
      content: () => loc({ getByRole: () => loc({ click: async () => undefined }) }),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesPeekOpenFirst("Archived beta");

    expect(presses).toContain("Enter");
    expect(url).toContain("peekCycle=c2");
  });

  it("throws when neither the press nor the keyboard opens a peek", async () => {
    const { page } = stubPage({
      url: "http://new.test/acme/projects/p1/archives/issues",
      content: () => loc(),
    });
    const driver = new WebNewDriver(page);

    await expect(driver.archivesPeekOpenFirst("Nowhere")).rejects.toThrow("opened no peek");
  });
});

describe("archivesPeekVisible and archivesPeekClose", () => {
  it("is false without a peek parameter, without reading the panel", async () => {
    const panel = loc({ count: async () => 0 });
    const { page } = stubPage({ url: "http://new.test/acme/projects/p1/archives/issues", panel });

    expect(await new WebNewDriver(page).archivesPeekVisible()).toBe(false);
  });

  it("reads the issue peek panel", async () => {
    const { page } = stubPage({
      url: "http://new.test/acme/projects/p1/archives/issues?peekIssueId=i1",
      panel: loc(),
    });

    expect(await new WebNewDriver(page).archivesPeekVisible()).toBe(true);
  });

  it("polls until the cycle peek panel mounts", async () => {
    let calls = 0;
    const { page } = stubPage({
      url: "http://new.test/acme/projects/p1/archives/cycles?peekCycle=c1",
      panel: loc({ isVisible: async () => ++calls > 2 }),
    });

    expect(await new WebNewDriver(page).archivesPeekVisible()).toBe(true);
  });

  it("gives up when the panel never shows", async () => {
    const { page } = stubPage({
      url: "http://new.test/acme/projects/p1/archives/modules?peekModule=m1",
      panel: loc({ isVisible: async () => false }),
    });

    expect(await new WebNewDriver(page).archivesPeekVisible()).toBe(false);
  });

  it("closes the issue peek through its panel button", async () => {
    let url = "http://new.test/acme/projects/p1/archives/issues?peekIssueId=i1";
    const { page } = stubPage({
      url: () => url,
      panel: loc({
        getByRole: () =>
          loc({
            click: async () => {
              url = "http://new.test/acme/projects/p1/archives/issues";
            },
          }),
      }),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesPeekClose();

    expect(url).not.toContain("peekIssueId");
  });

  it("closes a cycle peek through its panel button", async () => {
    let url = "http://new.test/acme/projects/p1/archives/cycles?peekCycle=c1";
    const { page } = stubPage({
      url: () => url,
      panel: loc({
        getByRole: () =>
          loc({
            click: async () => {
              url = "http://new.test/acme/projects/p1/archives/cycles";
            },
          }),
      }),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesPeekClose();

    expect(url).not.toContain("peekCycle");
  });

  it("falls back to Escape when the panel has no close control", async () => {
    let url = "http://new.test/acme/projects/p1/archives/modules?peekModule=m1";
    const { page, presses } = stubPage({
      url: () => url,
      press: (key) => {
        if (key === "Escape") url = "http://new.test/acme/projects/p1/archives/modules";
      },
      panel: loc({ getByRole: () => loc({ count: async () => 0 }) }),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesPeekClose();

    expect(presses).toContain("Escape");
    expect(url).not.toContain("peekModule");
  });

  it("does nothing when no peek is open", async () => {
    const { page, presses } = stubPage({
      url: "http://new.test/acme/projects/p1/archives/issues",
      panel: loc({ count: async () => 0 }),
    });

    await new WebNewDriver(page).archivesPeekClose();

    expect(presses).toEqual([]);
  });
});

describe("archivesSearchType and archivesSearchText", () => {
  it("fills the visible cycles search field", async () => {
    let filled = "";
    const input = loc({ fill: async (text: string) => void (filled = text) });
    const { page } = stubPage({
      url: "http://new.test/acme/projects/p1/archives/cycles",
      content: (access, value) => (access === "testid" && value === "archived-cycles-search-input" ? input : loc()),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesSearchType("alpha");

    expect(filled).toBe("alpha");
  });

  it("opens the modules search from its magnifier then fills", async () => {
    let expanded = false;
    let toggles = 0;
    let filled = "";
    const toggle = loc({
      click: async () => {
        toggles += 1;
        expanded = true;
      },
    });
    const input = loc({ isVisible: async () => expanded, fill: async (text: string) => void (filled = text) });
    const { page } = stubPage({
      url: "http://new.test/acme/projects/p1/archives/modules",
      content: (access, value) =>
        access === "testid" && value === "modules-search-input" ? input : access === "role" ? toggle : loc(),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesSearchType("beta");

    expect(toggles).toBe(1);
    expect(filled).toBe("beta");
  });

  it("opens the cycles search when its input is still unmounted", async () => {
    let expanded = false;
    let filled = "";
    const toggle = loc({ click: async () => void (expanded = true) });
    const input = loc({ isVisible: async () => expanded, fill: async (text: string) => void (filled = text) });
    const { page } = stubPage({
      url: "http://new.test/acme/projects/p1/archives/cycles",
      content: (access, value) =>
        access === "testid" && value === "archived-cycles-search-input" ? input : access === "role" ? toggle : loc(),
    });
    const driver = new WebNewDriver(page);

    await driver.archivesSearchType("gamma");

    expect(filled).toBe("gamma");
  });

  it("falls back to the header search field on the work-items tab", async () => {
    let filled = "";
    const input = loc({ fill: async (text: string) => void (filled = text) });
    const { page } = stubPage({
      url: "http://new.test/acme/projects/p1/archives/issues",
      content: (access, value) => (access === "placeholder" && value === "Search" ? input : loc()),
    });

    await new WebNewDriver(page).archivesSearchType("delta");

    expect(filled).toBe("delta");
  });

  it("reads the current search text", async () => {
    const input = loc({ inputValue: async () => "typed" });
    const { page } = stubPage({
      url: "http://new.test/acme/projects/p1/archives/cycles",
      content: (access, value) => (access === "testid" && value === "archived-cycles-search-input" ? input : loc()),
    });

    expect(await new WebNewDriver(page).archivesSearchText()).toBe("typed");
  });

  it("reads empty text when the field cannot be read", async () => {
    const input = loc({
      inputValue: async () => {
        throw new Error("detached");
      },
    });
    const { page } = stubPage({
      url: "http://new.test/acme/projects/p1/archives/modules",
      content: (access, value) => (access === "testid" && value === "modules-search-input" ? input : loc()),
    });

    expect(await new WebNewDriver(page).archivesSearchText()).toBe("");
  });
});
