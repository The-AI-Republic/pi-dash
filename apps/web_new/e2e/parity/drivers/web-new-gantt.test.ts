// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver timeline methods (NEWFRONT-624,
// ISS-044–049). A stub Page stands in for Playwright; every assertion is
// about the selector or value the method reads back, not about a browser.
import type { Locator, Page } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<unknown>;
  getAttribute?: (name: string) => Promise<string | null>;
  innerText?: () => Promise<string>;
  textContent?: () => Promise<string | null>;
  boundingBox?: () => Promise<{ x: number; y: number; width: number; height: number } | null>;
  evaluate?: (fn: (element: never, arg?: never) => unknown, arg?: unknown) => Promise<unknown>;
  inputValue?: () => Promise<string>;
  first?: () => StubLocator;
  nth?: (index: number) => StubLocator;
  getByRole?: (role: string, options?: unknown) => StubLocator;
  getByTestId?: (id: string) => StubLocator;
  getByLabel?: (label: string, options?: unknown) => StubLocator;
  locator?: (selector: string) => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 1,
    isVisible: async () => true,
    click: async () => undefined,
    waitFor: async () => undefined,
    getAttribute: async () => null,
    innerText: async () => "",
    textContent: async () => "",
    boundingBox: async () => null,
    evaluate: async () => null,
    inputValue: async () => "",
    ...overrides,
  };
  const defaults: StubLocator = {
    first: () => node,
    nth: () => node,
    getByRole: () => node,
    getByTestId: () => node,
    getByLabel: () => node,
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
    locator: () => locator(),
    waitForTimeout: async () => undefined,
    viewportSize: () => ({ width: 1280, height: 720 }),
    ...overrides,
  } as unknown as Page;
}

afterEach(() => {
  vi.useRealTimers();
});

describe("ganttOpenTimeline", () => {
  it("clicks the header switcher and waits for the chart", async () => {
    const clicks: string[] = [];
    const ganttSegment = locator({ click: async () => void clicks.push("gantt") });
    const shown = { value: false };
    const page = pageOf({
      getByTestId: (id: string) =>
        id === "layout-switcher-segmented"
          ? locator({ getByRole: () => locator({ first: () => locator({ waitFor: async () => undefined }) }) })
          : locator({ waitFor: async () => undefined }),
      locator: (selector: string) => {
        if (selector === '[data-layout="gantt_chart"]') {
          return locator({
            click: async () => {
              clicks.push("gantt");
              shown.value = true;
            },
            getAttribute: async () => null,
          });
        }
        if (selector === '[role="menuitem"], [role="menuitemradio"]') return locator({ count: async () => 0 });
        if (selector === "#gantt-container") return locator({ count: async () => (shown.value ? 1 : 0) });
        return locator();
      },
    });
    void ganttSegment;

    await new WebNewDriver(page).ganttOpenTimeline();

    expect(clicks).toEqual(["gantt"]);
  });

  it("uses the dropdown menu entry when the compact trigger is showing", async () => {
    const menuClicks: string[] = [];
    let opened = false;
    const page = pageOf({
      getByTestId: (id: string) => {
        if (id === "layout-switcher-segmented") return locator();
        if (id === "layout-switcher-dropdown") {
          return locator({
            getByRole: () => locator({ first: () => locator({ count: async () => 1, isVisible: async () => true }) }),
          });
        }
        return locator({ waitFor: async () => undefined });
      },
      locator: (selector: string) => {
        if (selector === '[data-layout="gantt_chart"]') return locator({ count: async () => 0 });
        if (selector === '[role="menuitem"], [role="menuitemradio"]')
          return locator({ count: async () => (opened ? 1 : 0) });
        if (selector === "#gantt-container") return locator({ count: async () => 1, isVisible: async () => opened });
        return locator();
      },
      getByRole: (_role: string, options?: { name?: string }) => {
        if (options?.name === "Timeline") {
          return locator({ click: async () => void menuClicks.push("Timeline") });
        }
        return locator();
      },
    });
    // The compact trigger opens the menu before the entry is clicked.
    page.getByTestId = ((id: string) => {
      if (id === "layout-switcher-segmented") return locator();
      if (id === "layout-switcher-dropdown") {
        return locator({
          getByRole: () =>
            locator({
              first: () =>
                locator({
                  count: async () => 1,
                  isVisible: async () => true,
                  click: async () => {
                    opened = true;
                  },
                }),
            }),
        });
      }
      return locator({ waitFor: async () => undefined });
    }) as Page["getByTestId"];

    await new WebNewDriver(page).ganttOpenTimeline();

    expect(menuClicks).toEqual(["Timeline"]);
  });
});

describe("ganttTimelineVisible", () => {
  it("reports false without a chart and the chart's visibility otherwise", async () => {
    const hidden = pageOf({ locator: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(hidden).ganttTimelineVisible()).resolves.toBe(false);
    const shown = pageOf({ locator: () => locator({ count: async () => 1, isVisible: async () => true }) });
    await expect(new WebNewDriver(shown).ganttTimelineVisible()).resolves.toBe(true);
  });
});

describe("boardActiveLayout", () => {
  function segmented(pressed: string[], keys: string[]) {
    const segments = locator({
      count: async () => pressed.length,
      nth: (index: number) =>
        locator({ getAttribute: async (name: string) => (name === "aria-pressed" ? pressed[index]! : keys[index]!) }),
    });
    return pageOf({
      getByTestId: () => locator({ getByRole: () => segments }),
    });
  }

  it("maps the pressed segment to its layout key", async () => {
    const page = segmented(["false", "false", "true"], ["list", "kanban", "calendar"]);
    await expect(new WebNewDriver(page).boardActiveLayout()).resolves.toBe("calendar");
  });

  it("returns list when nothing is pressed", async () => {
    await expect(new WebNewDriver(segmented(["false"], ["gantt_chart"])).boardActiveLayout()).resolves.toBe("list");
  });

  it("returns list when the pressed segment carries no known key", async () => {
    await expect(new WebNewDriver(segmented(["true"], ["mystery"])).boardActiveLayout()).resolves.toBe("list");
  });

  it("swallows attribute read failures", async () => {
    const segments = locator({
      count: async () => 1,
      nth: () =>
        locator({
          getAttribute: async () => {
            throw new Error("detached");
          },
        }),
    });
    const page = pageOf({ getByTestId: () => locator({ getByRole: () => segments }) });
    await expect(new WebNewDriver(page).boardActiveLayout()).resolves.toBe("list");
  });
});

describe("boardReloadIssues", () => {
  it("reloads and waits for the switcher", async () => {
    const reload = vi.fn(async () => undefined);
    const waitFor = vi.fn(async () => undefined);
    const page = pageOf({
      reload,
      getByTestId: () => locator({ getByRole: () => locator({ first: () => locator({ waitFor }) }) }),
    });
    await new WebNewDriver(page).boardReloadIssues();
    expect(reload).toHaveBeenCalledTimes(1);
    expect(waitFor).toHaveBeenCalledWith({ timeout: 30_000 });
  });
});

describe("issue peek", () => {
  it("detects the panel and reads its title", async () => {
    const page = pageOf({
      getByTestId: () => locator({ getByLabel: () => locator({ inputValue: async () => "Ship it" }) }),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.issuePeekVisible()).resolves.toBe(true);
    await expect(driver.issuePeekTitle()).resolves.toBe("Ship it");
  });

  it("reports no title while the panel is closed", async () => {
    const page = pageOf({ getByTestId: () => locator({ count: async () => 0 }) });
    const driver = new WebNewDriver(page);
    await expect(driver.issuePeekVisible()).resolves.toBe(false);
    await expect(driver.issuePeekTitle()).resolves.toBeNull();
  });

  it("closes the panel", async () => {
    const click = vi.fn(async () => undefined);
    let closed = false;
    const page = pageOf({
      getByRole: () => locator({ click }),
      getByTestId: () => locator({ isVisible: async () => !closed }),
    });
    click.mockImplementation(async () => {
      closed = true;
    });
    await new WebNewDriver(page).issuePeekClose();
    expect(click).toHaveBeenCalledWith({ timeout: 30_000 });
  });
});

describe("ganttHeader", () => {
  it("reads the count, views and both controls", async () => {
    const page = pageOf({
      getByTestId: (id: string) => {
        if (id === "gantt-header") {
          return locator({
            innerText: async () => "3 work items Week Month Quarter Today",
            getByRole: () => locator({ count: async () => 1, isVisible: async () => true }),
          });
        }
        return locator();
      },
    });
    await expect(new WebNewDriver(page).ganttHeader()).resolves.toEqual({
      count: 3,
      views: ["Week", "Month", "Quarter"],
      hasToday: true,
      hasFullscreen: true,
    });
  });

  it("reports a null count and hides unseen views", async () => {
    const page = pageOf({
      getByTestId: (id: string) =>
        id === "gantt-header"
          ? locator({
              innerText: async () => {
                throw new Error("detached");
              },
              getByRole: () => locator({ count: async () => 1, isVisible: async () => false }),
            })
          : locator({ count: async () => 0 }),
    });
    await expect(new WebNewDriver(page).ganttHeader()).resolves.toEqual({
      count: null,
      views: [],
      hasToday: false,
      hasFullscreen: false,
    });
  });
});

describe("ganttActiveZoom", () => {
  function zoomPage(first: string) {
    return pageOf({
      locator: () =>
        locator({
          count: async () => 1,
          first: () => locator({ innerText: async () => first }),
        }),
    });
  }

  it.each([
    ["Su 18", "Week"],
    ["7 - 13", "Month"],
    ["Oct", "Quarter"],
    ["?", "unknown"],
  ])("classifies %j as %s", async (label, expected) => {
    await expect(new WebNewDriver(zoomPage(label)).ganttActiveZoom()).resolves.toBe(expected);
  });

  it("returns unknown without header cells", async () => {
    const page = pageOf({ locator: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(page).ganttActiveZoom()).resolves.toBe("unknown");
  });

  it("returns unknown when the first cell cannot be read", async () => {
    const page = pageOf({
      locator: () =>
        locator({
          count: async () => 1,
          first: () =>
            locator({
              innerText: async () => {
                throw new Error("gone");
              },
            }),
        }),
    });
    await expect(new WebNewDriver(page).ganttActiveZoom()).resolves.toBe("unknown");
  });
});

describe("ganttSetZoom", () => {
  it("clicks the entry and settles once the offset stops moving", async () => {
    vi.useFakeTimers();
    const click = vi.fn(async () => undefined);
    const offsets = [100, 200, 200];
    let reads = 0;
    const page = pageOf({
      getByTestId: (id: string) =>
        id === "gantt-header" ? locator({ getByRole: () => locator({ first: () => locator({ click }) }) }) : locator(),
      locator: (selector: string) =>
        selector === '[data-tier="fine"] [data-segment-key]'
          ? locator({ count: async () => 1, first: () => locator({ innerText: async () => "Su 18" }) })
          : locator({
              first: () => locator({ evaluate: async () => offsets[Math.min(reads++, offsets.length - 1)] }),
            }),
      waitForTimeout: async () => undefined,
    });
    const driver = new WebNewDriver(page);
    const pending = driver.ganttSetZoom("Week");
    await vi.runAllTimersAsync();
    await pending;
    expect(click).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(reads).toBeGreaterThanOrEqual(2);
  });

  it("gives up after forty stable checks", async () => {
    vi.useFakeTimers();
    let reads = 0;
    const page = pageOf({
      getByTestId: () => locator(),
      locator: (selector: string) =>
        selector === '[data-tier="fine"] [data-segment-key]'
          ? locator({ count: async () => 1, first: () => locator({ innerText: async () => "Su 18" }) })
          : locator({ first: () => locator({ evaluate: async () => reads++ }) }),
      waitForTimeout: async () => undefined,
    });
    const pending = new WebNewDriver(page).ganttSetZoom("Week");
    await vi.runAllTimersAsync();
    await pending;
    expect(reads).toBe(41);
  });
});

describe("ganttDayWidth", () => {
  it("returns the day cell's own width at Week zoom", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-tier="fine"] [data-segment-key]'
          ? locator({
              count: async () => 1,
              first: () =>
                locator({
                  innerText: async () => "Su 18",
                  boundingBox: async () => ({ x: 0, y: 0, width: 40, height: 24 }),
                }),
            })
          : locator(),
    });
    await expect(new WebNewDriver(page).ganttDayWidth()).resolves.toBe(40);
  });

  it("divides a month cell by the days of that month", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-tier="fine"] [data-segment-key]'
          ? locator({
              count: async () => 1,
              first: () =>
                locator({
                  innerText: async () => "Oct",
                  boundingBox: async () => ({ x: 0, y: 0, width: 93, height: 24 }),
                  evaluate: async () => 31,
                  getAttribute: async () => "m-2026-9",
                }),
            })
          : locator(),
    });
    await expect(new WebNewDriver(page).ganttDayWidth()).resolves.toBeCloseTo(3);
  });

  it("divides a week cell by the days it spans at Month zoom", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-tier="fine"] [data-segment-key]'
          ? locator({
              count: async () => 1,
              first: () =>
                locator({
                  innerText: async () => "25 - 31",
                  boundingBox: async () => ({ x: 0, y: 0, width: 112, height: 24 }),
                  evaluate: async (fn: (element: never) => unknown) => fn({ textContent: "25 - 31" } as never),
                }),
            })
          : locator(),
    });
    await expect(new WebNewDriver(page).ganttDayWidth()).resolves.toBe(16);
  });

  it("treats an unreadable week label as seven days", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-tier="fine"] [data-segment-key]'
          ? locator({
              count: async () => 1,
              first: () =>
                locator({
                  innerText: async () => "25 - 31",
                  boundingBox: async () => ({ x: 0, y: 0, width: 112, height: 24 }),
                  evaluate: async (fn: (element: never) => unknown) => fn({ textContent: "?" } as never),
                }),
            })
          : locator(),
    });
    await expect(new WebNewDriver(page).ganttDayWidth()).resolves.toBe(16);
  });

  it("wraps a month-end week label", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-tier="fine"] [data-segment-key]'
          ? locator({
              count: async () => 1,
              first: () =>
                locator({
                  innerText: async () => "29 - 4",
                  boundingBox: async () => ({ x: 0, y: 0, width: 112, height: 24 }),
                  evaluate: async (fn: (element: never) => unknown) => fn({ textContent: "29 - 4" } as never),
                }),
            })
          : locator(),
    });
    await expect(new WebNewDriver(page).ganttDayWidth()).resolves.toBeCloseTo(112 / 6);
  });

  it("throws for an unknown zoom and a missing cell box", async () => {
    const unknown = pageOf({ locator: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(unknown).ganttDayWidth()).rejects.toThrow("unknown zoom");
    const noBox = pageOf({
      locator: (selector: string) =>
        selector === '[data-tier="fine"] [data-segment-key]'
          ? locator({
              count: async () => 1,
              first: () => locator({ innerText: async () => "Su 18", boundingBox: async () => null }),
            })
          : locator(),
    });
    await expect(new WebNewDriver(noBox).ganttDayWidth()).rejects.toThrow("week cell has no box");
  });
});

describe("ganttWeekendTinted", () => {
  it("needs both weekend and weekday columns", async () => {
    const page = pageOf({
      getByTestId: () => locator({ locator: () => locator({ count: async () => 0 }) }),
    });
    await expect(new WebNewDriver(page).ganttWeekendTinted()).resolves.toBe(false);
    const none = pageOf({ getByTestId: () => locator({ locator: () => locator({ count: async () => 0 }) }) });
    await expect(new WebNewDriver(none).ganttWeekendTinted()).resolves.toBe(false);
  });

  it("reports true when some but not all columns are tinted", async () => {
    const page = pageOf({
      getByTestId: (id: string) =>
        locator({
          locator: (selector: string) =>
            locator({
              count: async () => (id === "gantt-day-columns" ? (selector === '[data-weekend="true"]' ? 2 : 7) : 0),
            }),
        }),
    });
    await expect(new WebNewDriver(page).ganttWeekendTinted()).resolves.toBe(true);
  });

  it("reports false when every column is tinted", async () => {
    const page = pageOf({
      getByTestId: () => locator({ locator: () => locator({ count: async () => 7 }) }),
    });
    await expect(new WebNewDriver(page).ganttWeekendTinted()).resolves.toBe(false);
  });
});

describe("ganttWeekRowStarts", () => {
  const WEEK_CELLS = '[data-testid="gantt-day-columns"] [data-day][data-week-start="true"]';
  const FINE_CELLS = '[data-tier="fine"] [data-segment-key]';

  it("labels each week-start day cell with its weekday", async () => {
    const days = ["2026-10-18", "2026-10-24", "scratch"];
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === FINE_CELLS) {
          return locator({
            count: async () => 1,
            first: () => locator({ innerText: async () => "Su 18" }),
          });
        }
        if (selector === WEEK_CELLS) {
          return locator({
            count: async () => days.length,
            nth: (index: number) => locator({ getAttribute: async () => days[index]! }),
          });
        }
        return locator();
      },
    });
    const starts = await new WebNewDriver(page).ganttWeekRowStarts();
    // 2026-10-18 is a Sunday, 2026-10-24 a Saturday; the junk value reads
    // as an empty label instead of throwing.
    expect(starts).toEqual(["Su", "Sa", ""]);
  });

  it("reads week-start day cells, not every day of the week zoom", async () => {
    const queried: string[] = [];
    const page = pageOf({
      locator: (selector: string) => {
        queried.push(selector);
        if (selector === FINE_CELLS) {
          return locator({ count: async () => 1, first: () => locator({ innerText: async () => "We 21" }) });
        }
        if (selector === WEEK_CELLS) {
          return locator({
            count: async () => 1,
            nth: () => locator({ getAttribute: async () => "2026-10-21" }),
          });
        }
        return locator();
      },
    });
    await expect(new WebNewDriver(page).ganttWeekRowStarts()).resolves.toEqual(["We"]);
    expect(queried).toContain(WEEK_CELLS);
  });

  it("returns nothing outside Week zoom", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === FINE_CELLS
          ? locator({ count: async () => 1, first: () => locator({ innerText: async () => "Oct" }) })
          : locator(),
    });
    await expect(new WebNewDriver(page).ganttWeekRowStarts()).resolves.toEqual([]);
  });

  it("treats an unreadable day as an empty label", async () => {
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === FINE_CELLS) {
          return locator({ count: async () => 1, first: () => locator({ innerText: async () => "Su 18" }) });
        }
        if (selector === WEEK_CELLS) {
          return locator({
            count: async () => 1,
            nth: () =>
              locator({
                getAttribute: async () => {
                  throw new Error("detached");
                },
              }),
          });
        }
        return locator();
      },
    });
    await expect(new WebNewDriver(page).ganttWeekRowStarts()).resolves.toEqual([""]);
  });
});

describe("gantt today", () => {
  it("clicks Today and requires the marker to be visible", async () => {
    const click = vi.fn(async () => undefined);
    const page = pageOf({
      getByTestId: () =>
        locator({
          click,
          boundingBox: async () => ({ x: 10, y: 0, width: 40, height: 600 }),
        }),
    });
    await new WebNewDriver(page).ganttClickToday();
    expect(click).toHaveBeenCalledWith({ timeout: 30_000 });
  });

  it("measures the highlight against the viewport", async () => {
    const inside = pageOf({
      getByTestId: () => locator({ boundingBox: async () => ({ x: 10, y: 0, width: 40, height: 1 }) }),
    });
    await expect(new WebNewDriver(inside).ganttTodayVisible()).resolves.toBe(true);
    const behind = pageOf({
      getByTestId: () => locator({ boundingBox: async () => ({ x: -60, y: 0, width: 40, height: 1 }) }),
    });
    await expect(new WebNewDriver(behind).ganttTodayVisible()).resolves.toBe(false);
    const past = pageOf({
      getByTestId: () => locator({ boundingBox: async () => ({ x: 2000, y: 0, width: 40, height: 1 }) }),
    });
    await expect(new WebNewDriver(past).ganttTodayVisible()).resolves.toBe(false);
    const missing = pageOf({ getByTestId: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(missing).ganttTodayVisible()).resolves.toBe(false);
    const noBox = pageOf({ getByTestId: () => locator({ boundingBox: async () => null }) });
    await expect(new WebNewDriver(noBox).ganttTodayVisible()).resolves.toBe(false);
  });

  it("falls back to a 1280-wide viewport when none is reported", async () => {
    const page = pageOf({
      viewportSize: () => null,
      getByTestId: () => locator({ boundingBox: async () => ({ x: 1300, y: 0, width: 40, height: 1 }) }),
    });
    await expect(new WebNewDriver(page).ganttTodayVisible()).resolves.toBe(false);
  });

  it("reads today's marked header cell", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-tier="fine"] [data-today-segment="true"]' ? locator({ count: async () => 1 }) : locator(),
    });
    await expect(new WebNewDriver(page).ganttTodayHighlighted()).resolves.toBe(true);
  });
});

describe("gantt fullscreen", () => {
  it("toggles into the portal and back", async () => {
    let active = false;
    const page = pageOf({
      locator: () => locator({ count: async () => (active ? 1 : 0) }),
      getByTestId: () =>
        locator({
          click: async () => {
            active = !active;
          },
        }),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.ganttFullscreenActive()).resolves.toBe(false);
    await driver.ganttToggleFullscreen();
    await expect(driver.ganttFullscreenActive()).resolves.toBe(true);
  });
});

describe("gantt horizontal scroll", () => {
  it("rounds the strip width and reports zero without a box", async () => {
    const measured = pageOf({
      locator: () =>
        locator({ first: () => locator({ boundingBox: async () => ({ x: 0, y: 0, width: 2240.4, height: 36 }) }) }),
    });
    await expect(new WebNewDriver(measured).ganttTimelineWidth()).resolves.toBe(2240);
    const missing = pageOf({
      locator: () =>
        locator({
          first: () =>
            locator({
              boundingBox: async () => {
                throw new Error("gone");
              },
            }),
        }),
    });
    await expect(new WebNewDriver(missing).ganttTimelineWidth()).resolves.toBe(0);
  });

  it("reads and writes the container offset", async () => {
    const writes: number[] = [];
    let left = 120;
    const page = pageOf({
      locator: () =>
        locator({
          first: () =>
            locator({
              evaluate: async (fn: (element: { scrollLeft: number }, arg?: number) => unknown, arg?: number) =>
                fn(
                  {
                    get scrollLeft() {
                      return left;
                    },
                    set scrollLeft(value: number) {
                      left = value;
                      writes.push(value);
                    },
                  },
                  arg
                ),
            }),
        }),
      waitForTimeout: async () => undefined,
    });
    const driver = new WebNewDriver(page);
    await expect(driver.ganttScrollLeft()).resolves.toBe(120);
    await driver.ganttScrollTo(900);
    expect(writes).toEqual([900]);
  });
});

describe("gantt sidebar", () => {
  function rowsPage(names: string[], duration: string | null): Page {
    return pageOf({
      getByTestId: (id: string) => {
        if (id === "gantt-sidebar-row") {
          return locator({
            count: async () => names.length,
            nth: (index: number) =>
              locator({
                innerText: async () => `WEB-${index + 1} ${names[index]!}${duration === null ? "" : ` ${duration}`}`,
                getByTestId: () =>
                  locator({ count: async () => (duration === null ? 0 : 1), innerText: async () => duration ?? "" }),
              }),
          });
        }
        return locator();
      },
    });
  }

  it("splits identifier, name and duration", async () => {
    const rows = await new WebNewDriver(rowsPage(["First", "Second"], "3 days")).ganttSidebarRows();
    expect(rows).toEqual([
      { identifier: "WEB-1", name: "First", duration: "3 days" },
      { identifier: "WEB-2", name: "Second", duration: "3 days" },
    ]);
  });

  it("leaves duration null without a duration cell", async () => {
    const rows = await new WebNewDriver(rowsPage(["First"], null)).ganttSidebarRows();
    expect(rows[0]?.duration).toBeNull();
  });

  it("keeps a name without an identifier prefix", async () => {
    const page = pageOf({
      getByTestId: () =>
        locator({
          count: async () => 1,
          nth: () =>
            locator({ innerText: async () => "loose name", getByTestId: () => locator({ count: async () => 0 }) }),
        }),
    });
    await expect(new WebNewDriver(page).ganttSidebarRows()).resolves.toEqual([
      { identifier: null, name: "loose name", duration: null },
    ]);
  });

  it("returns names in sidebar order", async () => {
    await expect(new WebNewDriver(rowsPage(["First", "Second"], null)).ganttSidebarOrder()).resolves.toEqual([
      "First",
      "Second",
    ]);
  });

  function linksPage(texts: string[]): Page {
    return pageOf({
      locator: () =>
        locator({
          count: async () => texts.length,
          nth: (index: number) =>
            locator({
              innerText: async () => texts[index]!,
              getAttribute: async () => `issue-id-${index}`,
              click: async () => undefined,
            }),
        }),
      getByTestId: () =>
        locator({
          waitFor: async () => undefined,
          getByLabel: () => locator({ inputValue: async () => "Second" }),
        }),
    });
  }

  it("opens the peek of the row matching the name", async () => {
    await expect(
      new WebNewDriver(linksPage(["WEB-1 First", "WEB-2 Second"])).ganttOpenRowPeek("Second")
    ).resolves.toBeUndefined();
  });

  it("fails loudly when no row carries the name", async () => {
    await expect(new WebNewDriver(linksPage(["WEB-1 First"])).ganttOpenRowPeek("Second")).rejects.toThrow(
      "no timeline row titled"
    );
  });

  it("finds a bar by the row's issue id", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector.includes('a[id^="issue-"]')
          ? locator({
              count: async () => 1,
              nth: () => locator({ innerText: async () => "WEB-1 First", getAttribute: async () => "issue-row-1" }),
            })
          : locator({
              first: () =>
                locator({ count: async () => 1, boundingBox: async () => ({ x: 0, y: 0, width: 80, height: 24 }) }),
            }),
    });
    await expect(new WebNewDriver(page).ganttBarExists("First")).resolves.toBe(true);
  });

  it("fails when a row's link id is not an issue id", async () => {
    const page = pageOf({
      locator: () =>
        locator({
          count: async () => 1,
          nth: () => locator({ innerText: async () => "WEB-1 First", getAttribute: async () => "other" }),
        }),
    });
    await expect(new WebNewDriver(page).ganttBarExists("First")).rejects.toThrow("unexpected timeline row id");
  });
});

describe("gantt loading states", () => {
  it("reads the pulsing placeholders", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === "#gantt-sidebar .animate-pulse"
          ? locator({ count: async () => 1, last: () => locator({ isVisible: async () => true }) })
          : locator(),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.ganttSidebarLoading()).resolves.toBe(true);
    await expect(driver.ganttLoadMoreVisible()).resolves.toBe(true);
  });

  it("reports no loading placeholder when none is shown", async () => {
    const page = pageOf({ locator: () => locator({ count: async () => 0 }) });
    const driver = new WebNewDriver(page);
    await expect(driver.ganttSidebarLoading()).resolves.toBe(false);
    await expect(driver.ganttLoadMoreVisible()).resolves.toBe(false);
  });

  it("looks for the first-run heading", async () => {
    const page = pageOf({
      getByRole: () => locator({ count: async () => 1, first: () => locator({ isVisible: async () => true }) }),
    });
    await expect(new WebNewDriver(page).ganttEmptyVisible()).resolves.toBe(true);
    const empty = pageOf({ getByRole: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(empty).ganttEmptyVisible()).resolves.toBe(false);
  });
});

describe("gantt load delays", () => {
  /** A clock the stub waits advance, so the poll loops end quickly. */
  function stubClock(): { now: () => number; advance: (ms: number) => void } {
    let current = 1_000_000;
    const spy = vi.spyOn(Date, "now").mockImplementation(() => current);
    return {
      now: () => current,
      advance: (ms: number) => {
        current += ms;
        void spy;
      },
    };
  }

  afterEach(() => {
    vi.restoreAllMocks();
  });

  function delayedPage(loaderSeen: () => boolean, clock: { advance: (ms: number) => void }) {
    let handler: ((route: { continue: () => Promise<void> }) => Promise<void>) | null = null;
    return pageOf({
      route: async (_pattern: string, installed: typeof handler) => {
        handler = installed;
      },
      unroute: async () => undefined,
      reload: async () => undefined,
      waitForTimeout: async (ms: number) => clock.advance(ms),
      locator: (selector: string) =>
        selector === "div.animate-pulse"
          ? locator({
              count: async () => (loaderSeen() ? 1 : 0),
              first: () => locator({ isVisible: async () => loaderSeen() }),
            })
          : locator(),
      evaluate: async () => undefined,
      handler: () => handler,
    });
  }

  it("observes the layout loader while issues are delayed", async () => {
    const clock = stubClock();
    let seen = false;
    const page = delayedPage(() => seen, clock);
    const pending = new WebNewDriver(page).ganttLoadingObservedOnReload();
    await Promise.resolve();
    // First poll reports no loader; flip it on for the next poll.
    seen = true;
    await expect(pending).resolves.toBe(true);
  });

  it("gives up when the loader never appears", async () => {
    const clock = stubClock();
    const page = delayedPage(() => false, clock);
    await expect(new WebNewDriver(page).ganttLoadingObservedOnReload()).resolves.toBe(false);
  });

  it("observes the load-more placeholder after a reload", async () => {
    const clock = stubClock();
    let seen = false;
    const page = pageOf({
      route: async () => undefined,
      unroute: async () => undefined,
      reload: async () => undefined,
      waitForTimeout: async (ms: number) => clock.advance(ms),
      locator: (selector: string) =>
        selector === "#gantt-sidebar .animate-pulse"
          ? locator({ count: async () => (seen ? 1 : 0), last: () => locator({ isVisible: async () => seen }) })
          : locator({ first: () => locator({ waitFor: async () => undefined }), evaluate: async () => undefined }),
      getByTestId: () => locator({ getByRole: () => locator({ first: () => locator({ count: async () => 0 }) }) }),
    });
    const driver = new WebNewDriver(page);
    const open = vi.spyOn(driver, "ganttOpenTimeline").mockImplementation(async () => {
      seen = true;
    });
    await expect(driver.ganttLoadMoreObservedOnScroll()).resolves.toBe(true);
    expect(open).toHaveBeenCalledTimes(1);
  });

  it("gives up on load-more when the placeholder never shows", async () => {
    const clock = stubClock();
    const page = pageOf({
      route: async () => undefined,
      unroute: async () => undefined,
      reload: async () => undefined,
      waitForTimeout: async (ms: number) => clock.advance(ms),
      locator: () =>
        locator({
          count: async () => 0,
          last: () => locator({ isVisible: async () => false }),
          evaluate: async () => undefined,
          first: () => locator({ waitFor: async () => undefined }),
        }),
      getByTestId: () => locator({ getByRole: () => locator({ first: () => locator({ count: async () => 0 }) }) }),
    });
    const driver = new WebNewDriver(page);
    vi.spyOn(driver, "ganttOpenTimeline").mockImplementation(async () => undefined);
    await expect(driver.ganttLoadMoreObservedOnScroll()).resolves.toBe(false);
  });
});

describe("gantt quarter width", () => {
  it("divides a quarter's month cell by that month's day count", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-tier="fine"] [data-segment-key]'
          ? locator({
              count: async () => 1,
              first: () =>
                locator({
                  innerText: async () => "Oct",
                  boundingBox: async () => ({ x: 0, y: 0, width: 93, height: 24 }),
                  evaluate: async (fn: (element: { getAttribute: () => string | null }) => unknown) =>
                    fn({ getAttribute: () => "m-2026-9" } as never),
                }),
            })
          : locator(),
    });
    await expect(new WebNewDriver(page).ganttDayWidth()).resolves.toBe(3);
  });

  it("treats a key without a month as January", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-tier="fine"] [data-segment-key]'
          ? locator({
              count: async () => 1,
              first: () =>
                locator({
                  innerText: async () => "Jan",
                  boundingBox: async () => ({ x: 0, y: 0, width: 31, height: 24 }),
                  evaluate: async (fn: (element: { getAttribute: () => string | null }) => unknown) =>
                    fn({ getAttribute: () => "m-2026" } as never),
                }),
            })
          : locator(),
    });
    await expect(new WebNewDriver(page).ganttDayWidth()).resolves.toBe(1);
  });
});

describe("gantt driver reads behind absent markers", () => {
  it("keeps the switcher segment fallback when a key cannot be read", async () => {
    const segments = locator({
      count: async () => 1,
      nth: () =>
        locator({
          getAttribute: async (name: string) => {
            if (name === "aria-pressed") return "true";
            throw new Error("detached");
          },
        }),
    });
    const page = pageOf({ getByTestId: () => locator({ getByRole: () => segments }) });
    await expect(new WebNewDriver(page).boardActiveLayout()).resolves.toBe("list");
  });

  it("skips an invisible switcher candidate and keeps looking", async () => {
    let timeline = false;
    const page = pageOf({
      getByTestId: (id: string) => {
        if (id === "layout-switcher-segmented") return locator();
        if (id === "layout-switcher-dropdown") {
          return locator({
            getByRole: () => locator({ first: () => locator({ count: async () => 1, isVisible: async () => false }) }),
          });
        }
        return locator({ waitFor: async () => undefined });
      },
      locator: (selector: string) => {
        if (selector === '[data-layout="gantt_chart"]') {
          return locator({
            click: async () => {
              timeline = true;
            },
            getAttribute: async () => null,
          });
        }
        if (selector === '[role="menuitem"], [role="menuitemradio"]') return locator({ count: async () => 0 });
        if (selector === "#gantt-container")
          return locator({ count: async () => (timeline ? 1 : 0), isVisible: async () => timeline });
        return locator();
      },
    });
    await new WebNewDriver(page).ganttOpenTimeline();
    expect(timeline).toBe(true);
  });

  it("throws when the quarter cell has no box", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-tier="fine"] [data-segment-key]'
          ? locator({
              count: async () => 1,
              first: () =>
                locator({
                  innerText: async () => "Oct",
                  getAttribute: async () => "m-2026-9",
                  boundingBox: async () => null,
                }),
            })
          : locator(),
    });
    await expect(new WebNewDriver(page).ganttDayWidth()).rejects.toThrow("quarter cell has no box");
  });

  it("throws when the month week cell has no box", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-tier="fine"] [data-segment-key]'
          ? locator({
              count: async () => 1,
              first: () => locator({ innerText: async () => "25 - 31", boundingBox: async () => null }),
            })
          : locator(),
    });
    await expect(new WebNewDriver(page).ganttDayWidth()).rejects.toThrow("week cell has no box");
  });

  it("counts missing bar boxes as no bar", async () => {
    const page = pageOf({
      getByTestId: () =>
        locator({
          waitFor: async () => undefined,
          getByLabel: () => locator({ inputValue: async () => "First" }),
        }),
      locator: (selector: string) =>
        selector.includes('a[id^="issue-"]')
          ? locator({
              count: async () => 1,
              nth: () =>
                locator({
                  innerText: async () => "WEB-1 First",
                  getAttribute: async () => "issue-row-1",
                  getByLabel: () => locator({ inputValue: async () => "First" }),
                }),
            })
          : locator({ first: () => locator({ count: async () => 1, boundingBox: async () => null }) }),
    });
    await expect(new WebNewDriver(page).ganttBarExists("First")).resolves.toBe(false);
  });

  it("treats a missing bar locator as absent", async () => {
    const page = pageOf({
      getByTestId: () =>
        locator({
          waitFor: async () => undefined,
          getByLabel: () => locator({ inputValue: async () => "First" }),
        }),
      locator: (selector: string) =>
        selector.includes('a[id^="issue-"]')
          ? locator({
              count: async () => 1,
              nth: () =>
                locator({
                  innerText: async () => "WEB-1 First",
                  getAttribute: async () => "issue-row-1",
                  getByLabel: () => locator({ inputValue: async () => "First" }),
                }),
            })
          : locator({ first: () => locator({ count: async () => 0 }) }),
    });
    await expect(new WebNewDriver(page).ganttBarExists("First")).resolves.toBe(false);
  });

  it("falls back to a missing week label when the width query cannot parse the span", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-tier="fine"] [data-segment-key]'
          ? locator({
              count: async () => 1,
              first: () =>
                locator({
                  innerText: async () => "25 - 31",
                  boundingBox: async () => ({ x: 0, y: 0, width: 112, height: 24 }),
                  evaluate: async (fn: (element: { textContent: string }) => unknown) =>
                    fn({ textContent: "1 - x" } as never),
                }),
            })
          : locator(),
    });
    await expect(new WebNewDriver(page).ganttDayWidth()).resolves.toBe(16);
  });
});

describe("gantt driver route delays", () => {
  /** Capture the installed route handler once the method has awaited it. */
  async function installedHandler(method: (driver: WebNewDriver) => Promise<boolean>): Promise<{
    handler: () => (route: { continue: () => Promise<void> }) => Promise<void>;
    pending: Promise<boolean>;
  }> {
    // Each deadline check jumps a minute forward, so the polling loop ends
    // after a single pass instead of waiting out the real 120s deadline.
    let current = 1_000_000;
    vi.spyOn(Date, "now").mockImplementation(() => (current += 60_000));
    let handler: ((route: { continue: () => Promise<void> }) => Promise<void>) | null = null;
    const page = pageOf({
      route: async (_pattern: string, installed: typeof handler) => {
        handler = installed;
      },
      unroute: async () => undefined,
      reload: async () => undefined,
      waitForTimeout: async () => undefined,
      locator: () => locator({ count: async () => 0, first: () => locator({ isVisible: async () => false }) }),
    });
    const pending = method(new WebNewDriver(page));
    for (let attempt = 0; attempt < 10 && handler === null; attempt += 1) await Promise.resolve();
    if (handler === null) throw new Error("route handler was never installed");
    const captured = handler as (route: { continue: () => Promise<void> }) => Promise<void>;
    return { handler: () => captured, pending };
  }

  it("continues the delayed issue read and unroutes it afterwards", async () => {
    const { handler, pending } = await installedHandler((driver) => driver.ganttLoadingObservedOnReload());
    // The stub delays the read briefly; the route continues once it runs. The
    // delay is virtual, so the case spends no real seconds on it. Only the
    // timer functions are faked: the polling loop's deadline reads the
    // Date.now spy installed by installedHandler, which a faked Date would
    // freeze into an endless loop.
    const cont = vi.fn(async () => undefined);
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    try {
      const routed = handler()({ continue: cont });
      await vi.advanceTimersByTimeAsync(2_500);
      await routed;
      expect(cont).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
    await expect(pending).resolves.toBe(false);
  }, 15_000);

  it("survives a failed route continue and still unroutes", async () => {
    const { handler, pending } = await installedHandler((driver) => driver.ganttLoadingObservedOnReload());
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    try {
      const routed = handler()({
        continue: async () => {
          throw new Error("gone");
        },
      });
      await vi.advanceTimersByTimeAsync(2_500);
      await expect(routed).resolves.toBeUndefined();
    } finally {
      vi.useRealTimers();
    }
    await expect(pending).resolves.toBe(false);
  }, 10_000);

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("scrolls while waiting for load-more, then gives up at the deadline", async () => {
    let current = 1_000_000;
    vi.spyOn(Date, "now").mockImplementation(() => current);
    let handler: ((route: { continue: () => Promise<void> }) => Promise<void>) | null = null;
    let scrolled = 0;
    const page = pageOf({
      route: async (_pattern: string, installed: typeof handler) => {
        handler = installed;
      },
      unroute: async () => undefined,
      reload: async () => undefined,
      waitForTimeout: async () => {
        current += 60_000;
      },
      locator: (selector: string) =>
        selector === "#gantt-sidebar .animate-pulse"
          ? locator({ count: async () => 0, last: () => locator({ isVisible: async () => false }) })
          : locator({
              first: () =>
                locator({
                  evaluate: async (fn: (element: { scrollTop: number; scrollHeight: number }) => unknown) => {
                    const element = { scrollTop: 0, scrollHeight: 400 };
                    fn(element);
                    scrolled += 1;
                    if (element.scrollTop !== 400) throw new Error("scroll was not applied");
                    if (scrolled === 1) throw new Error("detached");
                  },
                }),
              waitFor: async () => undefined,
            }),
      getByTestId: () => locator({ getByRole: () => locator({ first: () => locator({ count: async () => 0 }) }) }),
    });
    const driver = new WebNewDriver(page);
    vi.spyOn(driver, "ganttOpenTimeline").mockImplementation(async () => undefined);
    await expect(driver.ganttLoadMoreObservedOnScroll()).resolves.toBe(false);
    expect(handler).not.toBeNull();
    const cont = vi.fn(async () => undefined);
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    try {
      const routed = handler!({ continue: cont });
      await vi.advanceTimersByTimeAsync(2_500);
      await routed;
    } finally {
      vi.useRealTimers();
    }
    expect(cont).toHaveBeenCalledTimes(1);
    // A failed scroll is swallowed, so the loop keeps polling.
    expect(scrolled).toBeGreaterThan(0);
  });

  it("tolerates rows whose text cannot be read", async () => {
    const page = pageOf({
      getByTestId: (id: string) =>
        id === "gantt-sidebar-row"
          ? locator({
              count: async () => 2,
              nth: (index: number) =>
                locator({
                  innerText: async () => {
                    if (index === 0) throw new Error("detached");
                    return "WEB-2 Second";
                  },
                  getByTestId: () =>
                    locator({ count: async () => 1, innerText: async () => (index === 0 ? "" : "2 days") }),
                }),
            })
          : locator({
              waitFor: async () => undefined,
              getByLabel: () => locator({ inputValue: async () => "Second" }),
            }),
    });
    const rows = await new WebNewDriver(page).ganttSidebarRows();
    expect(rows[0]).toEqual({ identifier: null, name: "", duration: null });
    expect(rows[1]).toEqual({ identifier: "WEB-2", name: "Second", duration: "2 days" });
  });

  it("skips sidebar links whose text cannot be read", async () => {
    const page = pageOf({
      getByTestId: () =>
        locator({ waitFor: async () => undefined, getByLabel: () => locator({ inputValue: async () => "Second" }) }),
      locator: (selector: string) =>
        selector.includes('a[id^="issue-"]')
          ? locator({
              count: async () => 2,
              nth: (index: number) =>
                locator({
                  innerText: async () => {
                    if (index === 0) throw new Error("detached");
                    return "WEB-2 Second";
                  },
                  click: async () => undefined,
                  getByLabel: () => locator({ inputValue: async () => "Second" }),
                }),
            })
          : locator({
              first: () =>
                locator({ count: async () => 1, boundingBox: async () => ({ x: 0, y: 0, width: 10, height: 10 }) }),
            }),
    });
    await expect(new WebNewDriver(page).ganttOpenRowPeek("Second")).resolves.toBeUndefined();
  });
});

describe("gantt sidebar reorder (ISS-050)", () => {
  interface FakeRow {
    name: string;
  }

  function fakePage(order: { rows: FakeRow[] }, options: { reorderOnDrop?: boolean } = {}) {
    const rowLocator = (index: number): StubLocator => {
      const name = order.rows[index]?.name ?? "";
      const link = locator({
        getAttribute: async (key: string) => (key === "aria-label" ? name : `issue-id-${name}`),
        innerText: async () => `WEB-${index + 1} ${name}`,
      });
      const node = locator({
        boundingBox: async () => ({ x: 0, y: index * 44, width: 200, height: 44 }),
        innerText: async () => `WEB-${index + 1} ${name}`,
        scrollIntoViewIfNeeded: async () => undefined,
        locator: () => link,
        getByTestId: () => locator({ count: async () => 0 }),
      });
      return node;
    };
    const page = pageOf({
      getByTestId: (id: string) =>
        id === "gantt-sidebar-row"
          ? locator({
              count: async () => order.rows.length,
              nth: (index: number) => rowLocator(index),
            })
          : locator(),
      locator: () => locator({ count: async () => 0 }),
      mouse: {
        move: async () => undefined,
        down: async () => undefined,
        up: async () => {
          if (options.reorderOnDrop !== false && order.rows.length > 1) {
            order.rows = [order.rows[1]!, order.rows[0]!, ...order.rows.slice(2)];
          }
        },
      },
    });
    return page;
  }

  it("drags a row before its target and polls the new order", async () => {
    const order = { rows: [{ name: "First" }, { name: "Second" }, { name: "Third" }] };
    await new WebNewDriver(fakePage(order)).ganttDragRowBefore("Second", "First");
    expect(order.rows.map((row) => row.name)).toEqual(["Second", "First", "Third"]);
  });

  it("attempts a drop without polling the order", async () => {
    const order = { rows: [{ name: "First" }, { name: "Second" }] };
    await expect(new WebNewDriver(fakePage(order)).ganttAttemptRowBefore("Second", "First")).resolves.toBeUndefined();
  });
});

describe("gantt bar gestures (ISS-051–053)", () => {
  const ISSUE = "123e4567-e89b-12d3-a456-426614174000";

  function gesturePage(options: { preview?: boolean } = {}) {
    const preview = options.preview ?? true;
    const handle = (label: string) =>
      locator({
        count: async () => 1,
        boundingBox: async () => ({ x: 100, y: 10, width: 12, height: 24 }),
        getAttribute: async () => label,
      });
    const bar: StubLocator = locator({
      count: async () => 1,
      boundingBox: async () => ({ x: 100, y: 10, width: 120, height: 24 }),
      scrollIntoViewIfNeeded: async () => undefined,
      getByTestId: (id: string) => {
        if (id === "gantt-resize-preview") {
          return locator({
            count: async () => (preview ? 1 : 0),
            isVisible: async () => preview,
            innerText: async () => "Oct 3, 2026",
          });
        }
        // The handle's own label carries the current date, so a driver that
        // read it would pass with no preview rendered at all.
        return handle(id === "gantt-resize-left" ? "Oct 3, 2026" : "Oct 5, 2026");
      },
    });
    const link = locator({
      count: async () => 1,
      getAttribute: async () => `issue-${ISSUE}`,
      innerText: async () => "WEB-1 First",
    });
    const page = pageOf({
      getByTestId: (id: string) => {
        if (id === "gantt-day-columns") {
          return locator({
            locator: () =>
              locator({
                first: () =>
                  locator({ count: async () => 1, boundingBox: async () => ({ x: 300, y: 0, width: 40, height: 44 }) }),
              }),
          });
        }
        return locator();
      },
      locator: (selector: string) => {
        if (selector === `#gantt-block-${ISSUE}`) return bar;
        if (selector === '#gantt-sidebar a[id^="issue-"]') return locator({ count: async () => 1, nth: () => link });
        return locator();
      },
      mouse: { move: async () => undefined, down: async () => undefined, up: async () => undefined },
    });
    return page;
  }

  it("drags a bar by whole days", async () => {
    await expect(new WebNewDriver(gesturePage()).ganttDragBar("First", 2)).resolves.toBeUndefined();
  });

  it("rejects a zero-day bar drag", async () => {
    await expect(new WebNewDriver(gesturePage()).ganttDragBar("First", 0)).rejects.toThrow(/nonzero/);
  });

  it("attempts a bar move without asserting the result", async () => {
    await expect(new WebNewDriver(gesturePage()).ganttAttemptBarMove("First", 1)).resolves.toBeUndefined();
  });

  it("resizes from either handle", async () => {
    await expect(new WebNewDriver(gesturePage()).ganttResizeBar("First", "left", 2)).resolves.toBeUndefined();
    await expect(new WebNewDriver(gesturePage()).ganttResizeBar("First", "right", -1)).resolves.toBeUndefined();
  });

  it("reads the floating preview date and handle visibility", async () => {
    const driver = new WebNewDriver(gesturePage());
    await expect(driver.ganttResizePreview("First", "left")).resolves.toBe("Oct 3, 2026");
    await expect(driver.ganttHandlesVisible("First")).resolves.toBe(true);
  });

  it("reports no preview when the chart renders none", async () => {
    // The handle label is still present; only the floating pill is absent.
    await expect(new WebNewDriver(gesturePage({ preview: false })).ganttResizePreview("First", "left")).resolves.toBe(
      null
    );
  });
});

describe("gantt add-block (ISS-054)", () => {
  const ISSUE = "123e4567-e89b-12d3-a456-426614174000";

  function addPage() {
    const link = locator({
      count: async () => 1,
      getAttribute: async (key: string) => (key === "aria-label" ? "Undated" : `issue-${ISSUE}`),
      innerText: async () => "WEB-2 Undated",
    });
    const row = locator({
      boundingBox: async () => ({ x: 0, y: 0, width: 200, height: 44 }),
      scrollIntoViewIfNeeded: async () => undefined,
      innerText: async () => "WEB-2 Undated",
      locator: () => link,
    });
    return pageOf({
      getByTestId: (id: string) => {
        if (id === "gantt-sidebar-row") return locator({ count: async () => 1, nth: () => row });
        if (id === "gantt-day-columns") {
          return locator({
            locator: () =>
              locator({
                first: () =>
                  locator({
                    count: async () => 1,
                    boundingBox: async () => ({ x: 300, y: 0, width: 400, height: 44 }),
                  }),
              }),
            first: () => locator({ boundingBox: async () => ({ x: 300, y: 0, width: 400, height: 44 }) }),
          });
        }
        return locator();
      },
      locator: (selector: string) => {
        if (selector === '#gantt-sidebar a[id^="issue-"]') return locator({ count: async () => 1, nth: () => link });
        if (selector === `#gantt-block-${ISSUE}`) {
          return locator({ count: async () => 1, boundingBox: async () => ({ x: 300, y: 0, width: 40, height: 24 }) });
        }
        return locator();
      },
      mouse: {
        move: async () => undefined,
        click: async () => undefined,
        down: async () => undefined,
        up: async () => undefined,
      },
    });
  }

  it("reports the add control on hover and plants a block", async () => {
    const driver = new WebNewDriver(addPage());
    await expect(driver.ganttRowAddVisible("Undated")).resolves.toBe(true);
    await expect(driver.ganttAddBlock("Undated", 2)).resolves.toBeUndefined();
  });
});

describe("gantt quick-add (ISS-055)", () => {
  const ISSUE = "123e4567-e89b-12d3-a456-426614174000";

  function quickPage() {
    const link = locator({
      count: async () => 1,
      getAttribute: async (key: string) => (key === "aria-label" ? "Fresh" : `issue-${ISSUE}`),
      innerText: async () => "WEB-1 Fresh",
    });
    const row = locator({
      boundingBox: async () => ({ x: 0, y: 0, width: 200, height: 44 }),
      scrollIntoViewIfNeeded: async () => undefined,
      innerText: async () => "WEB-1 Fresh",
      locator: () => link,
    });
    const field = locator({
      waitFor: async () => undefined,
      fill: async () => undefined,
      press: async () => undefined,
    });
    return pageOf({
      getByTestId: (id: string) =>
        id === "gantt-quick-add"
          ? locator({
              getByText: () => locator({ click: async () => undefined }),
              count: async () => 1,
              isVisible: async () => true,
            })
          : id === "gantt-sidebar-row"
            ? locator({ count: async () => 1, nth: () => row })
            : locator(),
      getByPlaceholder: () => field,
      locator: (selector: string) => {
        if (selector === '#gantt-sidebar a[id^="issue-"]') return locator({ count: async () => 1, nth: () => link });
        if (selector === `#gantt-block-${ISSUE}`) {
          return locator({ count: async () => 1, boundingBox: async () => ({ x: 300, y: 0, width: 40, height: 24 }) });
        }
        return locator();
      },
      mouse: { move: async () => undefined, down: async () => undefined, up: async () => undefined },
    });
  }

  it("opens the sticky entry, submits a title and sees the bar", async () => {
    await expect(new WebNewDriver(quickPage()).ganttQuickAdd("Fresh")).resolves.toBeUndefined();
  });

  it("reports whether the sticky entry is present", async () => {
    await expect(new WebNewDriver(quickPage()).ganttHasQuickAdd()).resolves.toBe(true);
    const absent = pageOf({ getByTestId: () => locator({ count: async () => 0, isVisible: async () => false }) });
    await expect(new WebNewDriver(absent).ganttHasQuickAdd()).resolves.toBe(false);
  });
});

describe("gantt bar chrome, preview and scroll arrows (ISS-056)", () => {
  const ISSUE = "row-1";
  const link = locator({ innerText: async () => "WEB-1 First", getAttribute: async () => `issue-${ISSUE}` });

  function barPage(bar: StubLocator, extra: Partial<Record<string, unknown>> = {}): Page {
    return pageOf({
      locator: (selector: string) => {
        if (selector === '#gantt-sidebar a[id^="issue-"]') return locator({ count: async () => 1, nth: () => link });
        if (selector === `#gantt-block-${ISSUE}`) return locator({ first: () => locator(bar) });
        return locator();
      },
      ...extra,
    });
  }

  it("reads bar tint, mask and pinned name off the block", async () => {
    const info = { tinted: true, masked: true, namePinned: true };
    const page = barPage({
      count: async () => 1,
      boundingBox: async () => ({ x: 0, y: 0, width: 80, height: 24 }),
      evaluate: async () => info,
    });
    await expect(new WebNewDriver(page).ganttBarInfo("First")).resolves.toEqual(info);
  });

  it("reads a missing bar or body as null", async () => {
    const missing = barPage({ count: async () => 0, boundingBox: async () => null });
    await expect(new WebNewDriver(missing).ganttBarInfo("First")).resolves.toBeNull();
    const bodiless = barPage({
      count: async () => 1,
      boundingBox: async () => ({ x: 0, y: 0, width: 80, height: 24 }),
      evaluate: async () => null,
    });
    await expect(new WebNewDriver(bodiless).ganttBarInfo("First")).resolves.toBeNull();
  });

  it("hovers the bar after scrolling it into view", async () => {
    const calls: string[] = [];
    const page = barPage({
      scrollIntoViewIfNeeded: async () => void calls.push("scroll"),
      hover: async () => void calls.push("hover"),
    });
    await new WebNewDriver(page).ganttHoverBar("First");
    expect(calls).toEqual(["scroll", "hover"]);
  });

  it("reports the hover preview card presence", async () => {
    const shown = pageOf({ getByTestId: () => locator({ count: async () => 1, isVisible: async () => true }) });
    await expect(new WebNewDriver(shown).ganttPreviewVisible()).resolves.toBe(true);
    const bare = pageOf({ getByTestId: () => locator({ count: async () => 0 }) });
    await expect(new WebNewDriver(bare).ganttPreviewVisible()).resolves.toBe(false);
  });

  it("opens peek from a bar click and settles on its title", async () => {
    const calls: string[] = [];
    const page = barPage(
      {
        scrollIntoViewIfNeeded: async () => undefined,
        click: async () => void calls.push("bar"),
      },
      {
        getByTestId: () =>
          locator({
            waitFor: async () => undefined,
            getByLabel: () => locator({ inputValue: async () => "First" }),
          }),
      }
    );
    await new WebNewDriver(page).ganttOpenBarPeek("First");
    expect(calls).toEqual(["bar"]);
  });

  it("reads the row's scroll arrow presence", async () => {
    const offered = pageOf({
      locator: () => locator({ getByRole: () => locator({ count: async () => 1, isVisible: async () => true }) }),
    });
    await expect(new WebNewDriver(offered).ganttScrollArrowVisible("First")).resolves.toBe(true);
    const bare = pageOf({
      locator: () => locator({ getByRole: () => locator({ count: async () => 0 }) }),
    });
    await expect(new WebNewDriver(bare).ganttScrollArrowVisible("First")).resolves.toBe(false);
  });

  it("refuses to click a missing scroll arrow", async () => {
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === '#gantt-sidebar a[id^="issue-"]') return locator({ count: async () => 1, nth: () => link });
        return locator({ getByRole: () => locator({ count: async () => 0 }) });
      },
    });
    await expect(new WebNewDriver(page).ganttClickScrollArrow("First")).rejects.toThrow("no scroll arrow");
  });

  it("clicks the arrow and settles once the bar is in view", async () => {
    const calls: string[] = [];
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === '#gantt-sidebar a[id^="issue-"]') return locator({ count: async () => 1, nth: () => link });
        if (selector === `#gantt-block-${ISSUE}`) {
          return locator({
            first: () => locator({ boundingBox: async () => ({ x: 10, y: 0, width: 80, height: 24 }) }),
          });
        }
        return locator({
          getByRole: () => locator({ click: async () => void calls.push("arrow") }),
        });
      },
    });
    await new WebNewDriver(page).ganttClickScrollArrow("First");
    expect(calls).toEqual(["arrow"]);
  });

  it("reads bar viewport membership off its box", async () => {
    const inside = barPage({ boundingBox: async () => ({ x: 10, y: 0, width: 80, height: 24 }) });
    await expect(new WebNewDriver(inside).ganttBarInView("First")).resolves.toBe(true);
    const left = barPage({ boundingBox: async () => ({ x: -100, y: 0, width: 80, height: 24 }) });
    await expect(new WebNewDriver(left).ganttBarInView("First")).resolves.toBe(false);
    const right = barPage({ boundingBox: async () => ({ x: 1300, y: 0, width: 80, height: 24 }) });
    await expect(new WebNewDriver(right).ganttBarInView("First")).resolves.toBe(false);
    const boxless = barPage({ boundingBox: async () => null });
    await expect(new WebNewDriver(boxless).ganttBarInView("First")).resolves.toBe(false);
  });
});
