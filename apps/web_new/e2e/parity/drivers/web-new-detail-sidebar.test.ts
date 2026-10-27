// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver detail-sidebar writes (NEWFRONT-658,
// ISS-148/159). A stub Page stands in for Playwright: each case pins
// the picker, banner, date, and label hooks the sidebar scenarios
// drive, so a markup change that breaks them fails here before the
// parity stack runs.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubNode {
  count?: () => Promise<number>;
  innerText?: () => Promise<string>;
  allTextContents?: () => Promise<string[]>;
  allInnerTexts?: () => Promise<string[]>;
  click?: (options?: unknown) => Promise<void>;
  fill?: (value: string, options?: unknown) => Promise<void>;
  hover?: (options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<void>;
  locator?: (selector: string) => Locator;
  getByTestId?: (id: string) => Locator;
  getByRole?: (role: string, options?: unknown) => Locator;
  getByLabel?: (label: string) => Locator;
  filter?: (options: unknown) => Locator;
  first?: () => Locator;
  last?: () => Locator;
  nth?: (index: number) => Locator;
  or?: (other: Locator) => Locator;
}

function node(overrides: Partial<StubNode> = {}): Locator {
  const self: StubNode = {
    count: async () => 1,
    innerText: async () => "",
    allTextContents: async () => [],
    allInnerTexts: async () => [],
    click: async () => undefined,
    fill: async () => undefined,
    hover: async () => undefined,
    waitFor: async () => undefined,
    ...overrides,
  };
  const chain: StubNode = {
    locator: () => node({ count: async () => 0 }),
    getByTestId: () => node({ count: async () => 0 }),
    getByRole: () => node({ count: async () => 0 }),
    getByLabel: () => node({ count: async () => 0 }),
    filter: () => self as unknown as Locator,
    first: () => self as unknown as Locator,
    last: () => self as unknown as Locator,
    nth: () => self as unknown as Locator,
    or: () => self as unknown as Locator,
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
    last: () => items[items.length - 1] ?? absent(),
    allTextContents: async () => {
      const out: string[] = [];
      for (const item of items) out.push(...(await item.allTextContents()));
      return out;
    },
  });
}

function item(text: string): Locator {
  return node({ innerText: async () => text, allTextContents: async () => [text] });
}

interface PageParts {
  testId?: (id: string) => Locator;
  role?: (role: string, options?: unknown) => Locator;
  label?: (label: string) => Locator;
  locator?: (selector: string) => Locator;
  keyboardPress?: (key: string) => Promise<void>;
}

function pageOf(parts: PageParts = {}): Page {
  return {
    getByTestId: (id: string) => parts.testId?.(id) ?? absent(),
    getByRole: (role: string, options?: unknown) => parts.role?.(role, options) ?? absent(),
    getByLabel: (label: string) => parts.label?.(label) ?? absent(),
    locator: (selector: string) => parts.locator?.(selector) ?? absent(),
    keyboard: { press: (key: string) => parts.keyboardPress?.(key) ?? Promise.resolve() },
  } as unknown as Page;
}

/** A sidebar group row whose trigger button records clicks. */
function groupRow(calls: string[], name: string): Locator {
  return node({
    getByRole: (role: string, options?: unknown) => {
      if (role !== "button") return absent();
      const want = (options as { name?: unknown } | undefined)?.name;
      if (want !== undefined && String(want) !== name && !(want instanceof RegExp)) return absent();
      return node({
        click: async () => {
          calls.push(`click:${name}`);
        },
      });
    },
    hover: async () => {
      calls.push(`hover:${name}`);
    },
  });
}

describe("DetailSidebarDriver", () => {
  it("picks a state through the row trigger and the option", async () => {
    const calls: string[] = [];
    const page = pageOf({
      role: (role: string, options?: unknown) => {
        if (role === "group") {
          const name = (options as { name?: string } | undefined)?.name;
          if (name === "State") return groupRow(calls, "State");
          return absent();
        }
        if (role === "listbox") {
          return node({
            getByRole: (inner: string) =>
              inner === "option"
                ? node({
                    click: async () => {
                      calls.push("click:option");
                    },
                  })
                : absent(),
          });
        }
        if (role === "combobox") return absent();
        return absent();
      },
      locator: () => absent(),
    });
    // waitForPickerOptions reads the listbox; pickerPick clicks the option.
    await new WebNewDriver(page).pickState("Todo");
    expect(calls).toContain("click:State");
    expect(calls).toContain("click:option");
  });

  it("clears the cycle by picking the No-cycle row", async () => {
    const picked: string[] = [];
    const page = pageOf({
      role: (role: string, options?: unknown) => {
        if (role === "group") {
          const name = (options as { name?: string } | undefined)?.name;
          if (name === "Cycle") {
            return node({
              getByRole: (inner: string) => (inner === "button" ? node({}) : absent()),
            });
          }
          return absent();
        }
        if (role === "listbox") {
          return node({
            getByRole: (inner: string, innerOptions?: unknown) => {
              if (inner !== "option") return absent();
              const want = (innerOptions as { name?: string } | undefined)?.name;
              return node({
                click: async () => {
                  picked.push(String(want));
                },
              });
            },
          });
        }
        return absent();
      },
    });
    await new WebNewDriver(page).clearCycle();
    expect(picked).toEqual(["No cycle"]);
  });

  it("dismisses the module menu after toggling", async () => {
    const keys: string[] = [];
    const page = pageOf({
      role: (role: string, options?: unknown) => {
        if (role === "group") {
          const name = (options as { name?: string } | undefined)?.name;
          if (name === "Modules") {
            return node({
              getByRole: (inner: string) => (inner === "button" ? node({}) : absent()),
            });
          }
          return absent();
        }
        if (role === "listbox") {
          return node({
            getByRole: (inner: string) => (inner === "option" ? node({}) : absent()),
          });
        }
        return absent();
      },
      keyboardPress: async (key: string) => {
        keys.push(key);
      },
    });
    await new WebNewDriver(page).toggleModule("Mod");
    expect(keys).toEqual(["Escape"]);
  });

  it("lists runs-on options with their reasons and closes the picker", async () => {
    const keys: string[] = [];
    const listbox = node({
      getByRole: (inner: string) => {
        if (inner !== "option") return absent();
        return node({ allInnerTexts: async () => ["PAR_pod_1", "Cloud run  not enabled"] });
      },
    });
    const page = pageOf({
      role: (role: string, options?: unknown) => {
        if (role === "group") {
          const name = (options as { name?: string } | undefined)?.name;
          if (name === "Runs on") {
            return node({
              getByRole: (inner: string) => (inner === "button" ? node({}) : absent()),
            });
          }
          return absent();
        }
        if (role === "listbox") return listbox;
        return absent();
      },
      keyboardPress: async (key: string) => {
        keys.push(key);
      },
    });
    await expect(new WebNewDriver(page).runsOnOptions()).resolves.toEqual(["PAR_pod_1", "Cloud run not enabled"]);
    expect(keys).toEqual(["Escape"]);
  });

  it("dismisses the assignee picker after picking", async () => {
    const keys: string[] = [];
    const calls: string[] = [];
    const page = pageOf({
      role: (role: string, options?: unknown) => {
        if (role === "group") {
          const name = (options as { name?: string } | undefined)?.name;
          if (name === "Assignees") {
            return node({
              getByRole: (inner: string) =>
                inner === "button"
                  ? node({
                      click: async () => {
                        calls.push("click:trigger");
                      },
                    })
                  : absent(),
            });
          }
          return absent();
        }
        if (role === "listbox") {
          return node({
            getByRole: (inner: string) =>
              inner === "option"
                ? node({
                    click: async () => {
                      calls.push("click:option");
                    },
                  })
                : absent(),
          });
        }
        return absent();
      },
      keyboardPress: async (key: string) => {
        keys.push(key);
      },
    });
    await new WebNewDriver(page).pickAssignee("You");
    expect(calls).toEqual(["click:trigger", "click:option"]);
    expect(keys).toEqual(["Escape"]);
  });

  it("picks an in-month day cell for a date row", async () => {
    const calls: string[] = [];
    const page = pageOf({
      role: (role: string, _options?: unknown) => {
        if (role === "group") {
          return node({
            getByRole: (inner: string) =>
              inner === "button"
                ? node({
                    click: async () => {
                      calls.push("click:trigger");
                    },
                  })
                : absent(),
          });
        }
        return absent();
      },
      locator: (selector: string) => {
        if (selector !== 'button[data-in-month="true"]') return absent();
        return node({
          filter: () =>
            node({
              click: async () => {
                calls.push("click:day");
              },
            }),
        });
      },
    });
    await new WebNewDriver(page).pickDate("Start date", "15");
    expect(calls).toEqual(["click:trigger", "click:day"]);
  });

  it("clears a date through the row's hover control", async () => {
    const calls: string[] = [];
    const page = pageOf({
      role: (role: string, _options?: unknown) => {
        if (role !== "group") return absent();
        return node({
          hover: async () => {
            calls.push("hover");
          },
          getByRole: (inner: string, innerOptions?: unknown) => {
            if (inner !== "button") return absent();
            const want = (innerOptions as { name?: string } | undefined)?.name;
            if (want !== "Clear Start date") return absent();
            return node({
              click: async () => {
                calls.push("click:clear");
              },
            });
          },
        });
      },
    });
    await new WebNewDriver(page).clearDate("Start date");
    expect(calls).toEqual(["hover", "click:clear"]);
  });

  it("sets the parent through the search dialog", async () => {
    const calls: string[] = [];
    const dialog = node({
      getByLabel: (label: string) =>
        label === "Search work items"
          ? node({
              fill: async (value: string) => {
                calls.push(`fill:${value}`);
              },
            })
          : absent(),
      getByRole: (inner: string) =>
        inner === "option"
          ? node({
              click: async () => {
                calls.push("click:option");
              },
            })
          : absent(),
    });
    const page = pageOf({
      role: (role: string, options?: unknown) => {
        if (role === "group") {
          const name = (options as { name?: string } | undefined)?.name;
          if (name === "Parent") {
            return node({
              getByRole: (inner: string) =>
                inner === "button"
                  ? node({
                      click: async () => {
                        calls.push("click:trigger");
                      },
                    })
                  : absent(),
            });
          }
          return absent();
        }
        if (role === "dialog") return dialog;
        return absent();
      },
    });
    await new WebNewDriver(page).setParentByName("Parent name");
    expect(calls).toEqual(["click:trigger", "fill:Parent name", "click:option"]);
  });

  it("reads the parent banner and null when absent", async () => {
    const banner = pageOf({
      testId: (id: string) =>
        id === "parent-banner" ? node({ innerText: async () => "PAR-1 Parent name" }) : absent(),
    });
    await expect(new WebNewDriver(banner).parentBanner("PAR-2")).resolves.toBe("PAR-1 Parent name");
    await expect(new WebNewDriver(pageOf()).parentBanner("PAR-2")).resolves.toBeNull();
  });

  it("lists the banner menu and leaves it open", async () => {
    const keys: string[] = [];
    let opened = false;
    const page = pageOf({
      role: () =>
        node({
          click: async () => {
            opened = true;
          },
        }),
      locator: (selector: string) =>
        selector === '[role="menuitem"]:visible'
          ? collection([item("PAR-9 Sibling"), item("Remove parent work item")])
          : absent(),
      keyboardPress: async (key: string) => {
        keys.push(key);
      },
    });
    await expect(new WebNewDriver(page).bannerMenuNames("PAR-2")).resolves.toEqual([
      "PAR-9 Sibling",
      "Remove parent work item",
    ]);
    expect(opened).toBe(true);
    expect(keys).toEqual([]);
  });

  it("retries the banner trigger when a re-render swallows the click", async () => {
    let clicks = 0;
    let waits = 0;
    const page = pageOf({
      role: () =>
        node({
          click: async () => {
            clicks += 1;
          },
        }),
      locator: (selector: string) => {
        if (selector !== '[role="menuitem"]:visible') return absent();
        return node({
          first: () =>
            node({
              waitFor: async () => {
                waits += 1;
                if (waits === 1) throw new Error("swallowed");
              },
            }),
          allTextContents: async () => ["Remove parent work item"],
        });
      },
    });
    await expect(new WebNewDriver(page).bannerMenuNames("PAR-2")).resolves.toEqual(["Remove parent work item"]);
    expect(clicks).toBe(2);
  });

  it("removes the parent through the banner menu", async () => {
    const calls: string[] = [];
    const page = pageOf({
      role: (role: string, options?: unknown) => {
        const want = (options as { name?: unknown } | undefined)?.name;
        if (want !== undefined && String(want) === "/Remove parent work item/i") {
          return node({
            click: async () => {
              calls.push("click:remove");
            },
          });
        }
        return node({
          click: async () => {
            calls.push(`click:${role}`);
          },
        });
      },
    });
    await new WebNewDriver(page).removeParent();
    expect(calls).toContain("click:remove");
  });

  it("opens the parent from the banner link and settles on the detail", async () => {
    const calls: string[] = [];
    const page = pageOf({
      testId: (id: string) =>
        id === "parent-banner"
          ? node({
              getByRole: (inner: string) =>
                inner === "link"
                  ? node({
                      click: async () => {
                        calls.push("click:link");
                      },
                    })
                  : absent(),
            })
          : absent(),
      role: (role: string) => (role === "heading" ? node({}) : absent()),
    });
    await new WebNewDriver(page).openParentFromBanner();
    expect(calls).toEqual(["click:link"]);
  });

  it("assigns an existing label from the filtered options", async () => {
    const calls: string[] = [];
    const page = pageOf({
      role: (role: string, _options?: unknown) => {
        if (role !== "group") return absent();
        return node({
          getByRole: (inner: string) =>
            inner === "button"
              ? node({
                  click: async () => {
                    calls.push("click:trigger");
                  },
                })
              : absent(),
        });
      },
      testId: (id: string) => {
        if (id === "label-picker-search") {
          return node({
            fill: async (value: string) => {
              calls.push(`fill:${value}`);
            },
          });
        }
        if (id === "label-picker-option") {
          return node({
            filter: () =>
              node({
                click: async () => {
                  calls.push("click:option");
                },
              }),
          });
        }
        return absent();
      },
    });
    await new WebNewDriver(page).addLabel("Existing");
    expect(calls).toEqual(["click:trigger", "fill:Existing", "click:option"]);
  });

  it("creates a label inline when no option matches", async () => {
    const calls: string[] = [];
    const page = pageOf({
      role: (role: string, _options?: unknown) => {
        if (role !== "group") return absent();
        return node({
          getByRole: (inner: string) => (inner === "button" ? node({}) : absent()),
        });
      },
      testId: (id: string) => {
        if (id === "label-picker-search") return node({});
        if (id === "label-picker-option") {
          return node({
            filter: () =>
              node({
                waitFor: async () => {
                  throw new Error("no match");
                },
              }),
          });
        }
        if (id === "label-picker-create") {
          return node({
            click: async () => {
              calls.push("click:create");
            },
          });
        }
        return absent();
      },
    });
    await new WebNewDriver(page).addLabel("Fresh");
    expect(calls).toEqual(["click:create"]);
  });

  it("removes a label through its chip", async () => {
    let clicked = "";
    const page = pageOf({
      role: (role: string, options?: unknown) => {
        if (role !== "button") return absent();
        const want = (options as { name?: string } | undefined)?.name;
        if (want !== "Remove Gone") return absent();
        return node({
          click: async () => {
            clicked = "chip";
          },
        });
      },
    });
    await new WebNewDriver(page).removeLabel("Gone");
    expect(clicked).toBe("chip");
  });
});
