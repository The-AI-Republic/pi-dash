// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver modules timeline methods (NEWFRONT-801,
// MOD-021). A stub Page stands in for Playwright; every assertion is about
// the selector, gesture or value the method reads back, not about a
// browser. The chart shell is the issues gantt's, so the gesture tests
// mirror the gantt driver's own stubs.
import type { Locator, Page } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  hover?: (options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<unknown>;
  scrollIntoViewIfNeeded?: (options?: unknown) => Promise<void>;
  getAttribute?: (name: string) => Promise<string | null>;
  innerText?: (options?: unknown) => Promise<string>;
  boundingBox?: () => Promise<{ x: number; y: number; width: number; height: number } | null>;
  evaluate?: (fn: (element: never, arg?: never) => unknown, arg?: unknown) => Promise<unknown>;
  first?: () => StubLocator;
  last?: () => StubLocator;
  nth?: (index: number) => StubLocator;
  filter?: (options?: unknown) => StubLocator;
  or?: (other: unknown) => StubLocator;
  getByRole?: (role: string, options?: unknown) => StubLocator;
  getByTestId?: (id: string) => StubLocator;
  getByText?: (text: string, options?: unknown) => StubLocator;
  locator?: (selector: string) => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 1,
    isVisible: async () => true,
    click: async () => undefined,
    hover: async () => undefined,
    waitFor: async () => undefined,
    scrollIntoViewIfNeeded: async () => undefined,
    getAttribute: async () => null,
    innerText: async () => "",
    boundingBox: async () => null,
    evaluate: async () => null,
    ...overrides,
  };
  const defaults: StubLocator = {
    first: () => node,
    last: () => node,
    nth: () => node,
    filter: () => node,
    or: () => node,
    getByRole: () => node,
    getByTestId: () => node,
    getByText: () => node,
    locator: () => node,
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
    getByText: () => locator(),
    locator: () => locator(),
    waitForTimeout: async () => undefined,
    viewportSize: () => ({ width: 1280, height: 720 }),
    mouse: { move: async () => undefined, down: async () => undefined, up: async () => undefined },
    ...overrides,
  } as unknown as Page;
}

afterEach(() => {
  vi.useRealTimers();
});

const MODULE_A = "323e4567-e89b-12d3-a456-426614174002";
const MODULE_B = "323e4567-e89b-12d3-a456-426614174003";

interface TimelineLink {
  name: string;
  href: string;
  glyphs: number;
}

function sidebarLinksPage(links: TimelineLink[], extra: Partial<Record<string, unknown>> = {}): Page {
  const link = (index: number): StubLocator => {
    const entry = links[index] ?? { name: "", href: "", glyphs: 0 };
    return {
      waitFor: async () => undefined,
      getAttribute: async (key: string) => {
        if (key === "aria-label") return entry.name;
        if (key === "href") return entry.href;
        return null;
      },
      locator: () => locator({ count: async () => entry.glyphs }),
    };
  };
  return pageOf({
    locator: (selector: string) => {
      if (selector === '#gantt-sidebar a[href*="/modules/"]') {
        return locator({ count: async () => links.length, nth: (index: number) => link(index) });
      }
      return locator();
    },
    ...extra,
  });
}

describe("modulesSetSort", () => {
  it("picks the label through the sort menu and waits for the trigger to follow", async () => {
    const clicks: string[] = [];
    const trigger = locator({ click: async () => void clicks.push("trigger") });
    const item = locator({
      click: async () => void clicks.push("item"),
      waitFor: async () => undefined,
    });
    const header = locator({
      getByTestId: (id: string) =>
        id === "modules-sort-trigger" ? trigger : locator({ innerText: async () => "Manual" }),
    });
    const page = pageOf({
      getByTestId: (id: string) => (id === "modules-list-header" ? header : locator()),
      getByRole: (role: string, options?: { name?: string }) => {
        expect(role).toBe("menuitem");
        expect(options).toMatchObject({ name: "Manual", exact: true });
        return item;
      },
    });
    await new WebNewDriver(page).modulesSetSort("Manual");
    expect(clicks).toEqual(["trigger", "item"]);
  });
});

describe("modulesTimelineRows", () => {
  it("reads names, hrefs and the status glyph per sidebar link", async () => {
    const page = sidebarLinksPage([
      { name: "Planned Bar", href: `/acme/projects/p/modules/${MODULE_A}`, glyphs: 1 },
      { name: "Dateless", href: `/acme/projects/p/modules/${MODULE_B}`, glyphs: 0 },
    ]);
    await expect(new WebNewDriver(page).modulesTimelineRows()).resolves.toEqual([
      { name: "Planned Bar", href: `/acme/projects/p/modules/${MODULE_A}`, statusGlyph: true },
      { name: "Dateless", href: `/acme/projects/p/modules/${MODULE_B}`, statusGlyph: false },
    ]);
  });

  it("reads no rows when the sidebar is empty", async () => {
    await expect(new WebNewDriver(sidebarLinksPage([])).modulesTimelineRows()).resolves.toEqual([]);
  });
});

describe("modulesTimeline link resolution", () => {
  it("rejects an unknown module name", async () => {
    const page = sidebarLinksPage([{ name: "Planned Bar", href: "/acme/projects/p/modules/x", glyphs: 1 }]);
    await expect(new WebNewDriver(page).modulesTimelineBarExists("Nobody")).rejects.toThrow(
      /no modules timeline row titled "Nobody"/
    );
  });

  it("rejects a row whose href carries no module id", async () => {
    const page = sidebarLinksPage([{ name: "Planned Bar", href: "/acme/projects/p/modules", glyphs: 1 }]);
    await expect(new WebNewDriver(page).modulesTimelineBarExists("Planned Bar")).rejects.toThrow(/no module href/);
  });
});

describe("modulesTimelineBarColor", () => {
  function barPage(color: string): Page {
    const inner = locator({ waitFor: async () => undefined, evaluate: async () => color });
    const bar = locator({ waitFor: async () => undefined, getByTestId: () => inner });
    const links = [{ name: "Planned Bar", href: `/acme/projects/p/modules/${MODULE_A}`, glyphs: 1 }];
    const link = locator({
      waitFor: async () => undefined,
      getAttribute: async (key: string) =>
        key === "aria-label" ? "Planned Bar" : key === "href" ? links[0]!.href : null,
      locator: () => locator({ count: async () => 1 }),
    });
    return pageOf({
      locator: (selector: string) => {
        if (selector === '#gantt-sidebar a[href*="/modules/"]') {
          return locator({ count: async () => 1, nth: () => link });
        }
        if (selector === `#gantt-block-${MODULE_A}`) return bar;
        return locator();
      },
    });
  }

  it("normalizes a rendered rgb color to hex", async () => {
    await expect(new WebNewDriver(barPage("rgb(63, 118, 255)")).modulesTimelineBarColor("Planned Bar")).resolves.toBe(
      "#3f76ff"
    );
  });

  it("passes hex colors through in lowercase", async () => {
    const driver = new WebNewDriver(barPage("#16A34A"));
    await expect(driver.modulesTimelineBarColor("Planned Bar")).resolves.toBe("#16a34a");
    await expect(new WebNewDriver(barPage("#ABC")).modulesTimelineBarColor("Planned Bar")).resolves.toBe("#aabbcc");
  });

  it("rejects a color it cannot read", async () => {
    await expect(new WebNewDriver(barPage("nonsense")).modulesTimelineBarColor("Planned Bar")).rejects.toThrow(
      /cannot read a bar color/
    );
  });
});

describe("modulesTimelineHoverBar", () => {
  it("brings the bar into view before hovering it", async () => {
    const calls: string[] = [];
    const bar = locator({
      scrollIntoViewIfNeeded: async () => void calls.push("scroll"),
      hover: async () => void calls.push("hover"),
    });
    const links = [{ name: "Planned Bar", href: `/acme/projects/p/modules/${MODULE_A}`, glyphs: 1 }];
    const link = locator({
      waitFor: async () => undefined,
      getAttribute: async (key: string) =>
        key === "aria-label" ? "Planned Bar" : key === "href" ? links[0]!.href : null,
      locator: () => locator(),
    });
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === '#gantt-sidebar a[href*="/modules/"]') {
          return locator({ count: async () => 1, nth: () => link });
        }
        expect(selector).toBe(`#gantt-block-${MODULE_A}`);
        return bar;
      },
    });
    await new WebNewDriver(page).modulesTimelineHoverBar("Planned Bar");
    expect(calls).toEqual(["scroll", "hover"]);
  });
});

describe("modulesTimelineTooltip", () => {
  it("reads the hovered bar's title", async () => {
    const page = pageOf({
      locator: (selector: string) => {
        expect(selector).toBe('[data-testid="gantt-bar"]:hover');
        return locator({ count: async () => 1, getAttribute: async () => "Planned Bar: Sep 01 to Sep 10" });
      },
    });
    await expect(new WebNewDriver(page).modulesTimelineTooltip()).resolves.toBe("Planned Bar: Sep 01 to Sep 10");
  });

  it("reads nothing when no bar is hovered", async () => {
    const page = pageOf({ locator: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(page).modulesTimelineTooltip()).resolves.toBeNull();
  });
});

describe("modulesTimelineBarExists", () => {
  function existsPage(box: { x: number; y: number; width: number; height: number } | null, present: boolean): Page {
    const bar = locator({ count: async () => (present ? 1 : 0), boundingBox: async () => box });
    const link = locator({
      waitFor: async () => undefined,
      getAttribute: async (key: string) =>
        key === "aria-label" ? "Planned Bar" : key === "href" ? `/acme/projects/p/modules/${MODULE_A}` : null,
      locator: () => locator(),
    });
    return pageOf({
      locator: (selector: string) => {
        if (selector === '#gantt-sidebar a[href*="/modules/"]') {
          return locator({ count: async () => 1, nth: () => link });
        }
        return bar;
      },
    });
  }

  it("reports a rendered bar and rejects the rest", async () => {
    const driver = (page: Page): Promise<boolean> => new WebNewDriver(page).modulesTimelineBarExists("Planned Bar");
    await expect(driver(existsPage({ x: 0, y: 0, width: 120, height: 24 }, true))).resolves.toBe(true);
    await expect(driver(existsPage(null, false))).resolves.toBe(false);
    await expect(driver(existsPage(null, true))).resolves.toBe(false);
    await expect(driver(existsPage({ x: 0, y: 0, width: 0, height: 24 }, true))).resolves.toBe(false);
  });
});

describe("modulesTimeline bar gestures", () => {
  const DAY_WIDTH = 40;
  const BOX = { x: 100, y: 10, width: 120, height: 24 };

  function gesturePage(call: { moves: { x: number; y: number }[] }, handleBox: typeof BOX | null = null): Page {
    const link = locator({
      waitFor: async () => undefined,
      getAttribute: async (key: string) =>
        key === "aria-label" ? "Planned Bar" : key === "href" ? `/acme/projects/p/modules/${MODULE_A}` : null,
      locator: () => locator(),
    });
    const bar = locator({
      scrollIntoViewIfNeeded: async () => undefined,
      boundingBox: async () => BOX,
      getByTestId: (id: string) => {
        expect(id.startsWith("gantt-resize-")).toBe(true);
        return locator({ waitFor: async () => undefined, boundingBox: async () => handleBox });
      },
    });
    return pageOf({
      getByTestId: (id: string) => {
        if (id !== "gantt-day-columns") return locator();
        return locator({
          locator: () =>
            locator({
              first: () =>
                locator({
                  count: async () => 1,
                  boundingBox: async () => ({
                    x: 0,
                    y: 0,
                    width: DAY_WIDTH,
                    height: 24,
                  }),
                }),
            }),
        });
      },
      locator: (selector: string) => {
        if (selector === '#gantt-sidebar a[href*="/modules/"]') {
          return locator({ count: async () => 1, nth: () => link });
        }
        return bar;
      },
      mouse: {
        move: async (x: number, y: number) => void call.moves.push({ x, y }),
        down: async () => undefined,
        up: async () => undefined,
      },
      waitForTimeout: async () => undefined,
    });
  }

  it("drags a bar by whole day widths", async () => {
    const call = { moves: [] as { x: number; y: number }[] };
    await new WebNewDriver(gesturePage(call)).modulesTimelineDragBar("Planned Bar", 2);
    expect(call.moves).toEqual([
      { x: 160, y: 22 },
      { x: 240, y: 22 },
    ]);
  });

  it("rejects a zero-day bar drag or resize", async () => {
    const call = { moves: [] as { x: number; y: number }[] };
    const driver = new WebNewDriver(gesturePage(call));
    await expect(driver.modulesTimelineDragBar("Planned Bar", 0)).rejects.toThrow(/nonzero day delta/);
    await expect(driver.modulesTimelineResizeBar("Planned Bar", "left", 0)).rejects.toThrow(/nonzero day delta/);
    await expect(driver.modulesTimelineAttemptBarMove("Planned Bar", 0)).rejects.toThrow(/nonzero day delta/);
  });

  it("resizes from either handle edge", async () => {
    const left = { moves: [] as { x: number; y: number }[] };
    await new WebNewDriver(gesturePage(left, BOX)).modulesTimelineResizeBar("Planned Bar", "left", 1);
    expect(left.moves).toEqual([
      { x: 102, y: 22 },
      { x: 142, y: 22 },
    ]);
    const right = { moves: [] as { x: number; y: number }[] };
    await new WebNewDriver(gesturePage(right, BOX)).modulesTimelineResizeBar("Planned Bar", "right", -1);
    expect(right.moves).toEqual([
      { x: 218, y: 22 },
      { x: 178, y: 22 },
    ]);
  });

  it("rejects an unmeasurable resize handle", async () => {
    const call = { moves: [] as { x: number; y: number }[] };
    await expect(
      new WebNewDriver(gesturePage(call, null)).modulesTimelineResizeBar("Planned Bar", "left", 1)
    ).rejects.toThrow(/resize handle has no box/);
  });

  it("attempts a bar move without asserting the outcome", async () => {
    const call = { moves: [] as { x: number; y: number }[] };
    await new WebNewDriver(gesturePage(call)).modulesTimelineAttemptBarMove("Planned Bar", -2);
    expect(call.moves).toEqual([
      { x: 160, y: 22 },
      { x: 80, y: 22 },
    ]);
  });
});

describe("modulesTimelineOrder", () => {
  it("reads the sidebar names in order and skips blanks", async () => {
    const page = sidebarLinksPage([
      { name: "Alpha", href: "/a", glyphs: 1 },
      { name: "  ", href: "/b", glyphs: 0 },
      { name: "Bravo", href: "/c", glyphs: 1 },
    ]);
    await expect(new WebNewDriver(page).modulesTimelineOrder()).resolves.toEqual(["Alpha", "Bravo"]);
    await expect(new WebNewDriver(page).modulesTimelineRowNames()).resolves.toEqual(["Alpha", "Bravo"]);
    await expect(new WebNewDriver(page).modulesListTimelineNames()).resolves.toEqual(["Alpha", "Bravo"]);
  });
});

describe("modulesTimeline row drags", () => {
  function dragPage(order: string[]): Page {
    const rows = order.map((name, index) => ({
      link: locator({
        count: async () => 1,
        getAttribute: async (key: string) => (key === "aria-label" ? name : `/modules/${name}`),
        boundingBox: async () => ({ x: 0, y: index * 44, width: 296, height: 44 }),
        waitFor: async () => undefined,
      }),
      row: locator({
        scrollIntoViewIfNeeded: async () => undefined,
        boundingBox: async () => ({ x: 0, y: order.indexOf(name) * 44, width: 296, height: 44 }),
      }),
    }));
    for (const entry of rows) {
      (entry.row as unknown as StubLocator).locator = () => entry.link;
    }
    return pageOf({
      getByTestId: (id: string) => {
        expect(id).toBe("gantt-sidebar-row");
        return locator({ count: async () => rows.length, nth: (index: number) => rows[index]!.row });
      },
      locator: (selector: string) => {
        expect(selector).toBe('#gantt-sidebar a[href*="/modules/"]');
        return locator({
          count: async () => order.length,
          nth: (index: number) => locator({ getAttribute: async () => order[index] ?? null }),
        });
      },
      mouse: {
        move: async () => undefined,
        down: async () => undefined,
        up: async () => {
          const moved = order.shift();
          if (moved !== undefined) order.push(moved);
        },
      },
      waitForTimeout: async () => undefined,
    });
  }

  it("drags a row before another and waits for the new adjacency", async () => {
    const order = ["Alpha", "Bravo", "Charlie"];
    await new WebNewDriver(dragPage(order)).modulesTimelineDragRowBefore("Bravo", "Charlie");
    expect(order).toEqual(["Bravo", "Charlie", "Alpha"]);
  });

  it("attempts a row drag without asserting the outcome", async () => {
    const order = ["Alpha", "Bravo", "Charlie"];
    await new WebNewDriver(dragPage(order)).modulesTimelineAttemptRowBefore("Bravo", "Charlie");
    expect(order).toEqual(["Bravo", "Charlie", "Alpha"]);
  });
});

describe("modulesXTimelineBarsPresent", () => {
  function barsPage(content: number, bars: number): Page {
    return pageOf({
      getByTestId: (id: string) => {
        if (id === "modules-list") {
          return locator({
            count: async () => content,
            getByTestId: () => locator({ count: async () => bars }),
          });
        }
        return locator({ count: async () => 0 });
      },
    });
  }

  it("reports bars only when the list shows them", async () => {
    await expect(new WebNewDriver(barsPage(0, 0)).modulesXTimelineBarsPresent()).resolves.toBe(false);
    await expect(new WebNewDriver(barsPage(1, 0)).modulesXTimelineBarsPresent()).resolves.toBe(false);
    await expect(new WebNewDriver(barsPage(1, 2)).modulesXTimelineBarsPresent()).resolves.toBe(true);
  });
});

describe("modulesManualSortWarningVisible", () => {
  function warningPage(count: number, visible: boolean): Page {
    return pageOf({
      getByText: (text: string) => {
        expect(text).toBe("Manual placement required");
        return locator({ count: async () => count, isVisible: async () => visible });
      },
    });
  }

  it("reports the manual warning only while its toast shows", async () => {
    await expect(new WebNewDriver(warningPage(1, true)).modulesManualSortWarningVisible()).resolves.toBe(true);
    await expect(new WebNewDriver(warningPage(0, false)).modulesManualSortWarningVisible()).resolves.toBe(false);
    await expect(new WebNewDriver(warningPage(1, false)).modulesManualSortWarningVisible()).resolves.toBe(false);
  });
});

describe("modulesAwaitRow timeline branch", () => {
  it("also waits on the timeline sidebar row", async () => {
    const seen: string[] = [];
    const classic = locator({
      filter: () =>
        locator({
          or: (other: unknown) =>
            locator({
              first: () =>
                locator({
                  waitFor: async () => void seen.push(`or:${String((other as { __tag?: string }).__tag ?? "")}`),
                }),
            }),
        }),
    });
    const timeline = locator({ filter: () => locator({ __tag: "timeline" }) });
    const page = pageOf({
      locator: (selector: string) => {
        seen.push(`css:${selector}`);
        return classic;
      },
      getByTestId: (id: string) => {
        seen.push(`testid:${id}`);
        return timeline;
      },
      getByRole: (role: string, options?: { name?: RegExp }) => {
        seen.push(`role:${role}:${String(options?.name)}`);
        return locator();
      },
    });
    await new WebNewDriver(page).modulesAwaitRow("Alpha");
    expect(seen).toContain('css:[data-testid="modules-row"], [data-testid="modules-board-card"]');
    expect(seen).toContain("testid:gantt-sidebar-row");
    expect(seen).toContain("role:link:/^Alpha$/");
    expect(seen).toContain("or:timeline");
  });
});
