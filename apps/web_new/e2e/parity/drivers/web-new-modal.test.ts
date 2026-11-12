// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver modal reads and writes (NEWFRONT-658,
// ISS-117/136). A stub Page stands in for Playwright: each case pins the
// create dialog, git section, parent search, row menu, and pager hooks
// the modal scenarios drive, so a markup change that breaks them fails
// here before the parity stack runs.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubNode {
  count?: () => Promise<number>;
  textContent?: () => Promise<string | null>;
  inputValue?: () => Promise<string>;
  click?: (options?: unknown) => Promise<void>;
  fill?: (value: string, options?: unknown) => Promise<void>;
  hover?: (options?: unknown) => Promise<void>;
  press?: (key: string, options?: unknown) => Promise<void>;
  pressSequentially?: (text: string, options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<void>;
  isVisible?: () => Promise<boolean>;
  isDisabled?: () => Promise<boolean>;
  innerText?: () => Promise<string>;
  allInnerTexts?: () => Promise<string[]>;
  evaluate?: (fn: (element: never) => unknown) => Promise<unknown>;
  locator?: (selector: string) => Locator;
  getByTestId?: (id: string) => Locator;
  getByRole?: (role: string, options?: unknown) => Locator;
  getByLabel?: (label: string) => Locator;
  getByPlaceholder?: (text: string) => Locator;
  filter?: (options: unknown) => Locator;
  first?: () => Locator;
  nth?: (index: number) => Locator;
  or?: (other: Locator) => Locator;
}

function node(overrides: Partial<StubNode> = {}): Locator {
  const self: StubNode = {
    count: async () => 1,
    textContent: async () => "",
    inputValue: async () => "",
    click: async () => undefined,
    fill: async () => undefined,
    hover: async () => undefined,
    press: async () => undefined,
    pressSequentially: async () => undefined,
    waitFor: async () => undefined,
    isVisible: async () => true,
    isDisabled: async () => false,
    innerText: async () => "",
    allInnerTexts: async () => [],
    evaluate: async () => undefined,
    ...overrides,
  };
  const chain: StubNode = {
    locator: () => node({ count: async () => 0 }),
    getByTestId: () => node({ count: async () => 0 }),
    getByRole: () => node({ count: async () => 0 }),
    getByLabel: () => node({ count: async () => 0 }),
    getByPlaceholder: () => node({ count: async () => 0 }),
    filter: () => self as unknown as Locator,
    first: () => self as unknown as Locator,
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

interface PageParts {
  testId?: (id: string) => Locator;
  role?: (role: string, options?: unknown) => Locator;
  label?: (label: string) => Locator;
  placeholder?: (text: string) => Locator;
  locator?: (selector: string) => Locator;
  goto?: (url: string, options?: unknown) => Promise<void>;
  waitForTimeout?: (ms: number) => Promise<void>;
  waitForLoadState?: (state?: unknown) => Promise<void>;
  reload?: () => Promise<void>;
  evaluate?: (fn: () => unknown) => Promise<unknown>;
  keyboardPress?: (key: string) => Promise<void>;
  keyboardType?: (text: string) => Promise<void>;
}

function pageOf(parts: PageParts = {}): Page {
  return {
    getByTestId: (id: string) => parts.testId?.(id) ?? absent(),
    getByRole: (role: string, options?: unknown) => parts.role?.(role, options) ?? absent(),
    getByLabel: (label: string) => parts.label?.(label) ?? absent(),
    getByPlaceholder: (text: string) => parts.placeholder?.(text) ?? absent(),
    locator: (selector: string) => parts.locator?.(selector) ?? absent(),
    goto: (url: string, options?: unknown) => parts.goto?.(url, options) ?? Promise.resolve(),
    waitForTimeout: (ms: number) => parts.waitForTimeout?.(ms) ?? Promise.resolve(),
    waitForLoadState: (state?: unknown) => parts.waitForLoadState?.(state) ?? Promise.resolve(),
    reload: () => parts.reload?.() ?? Promise.resolve(),
    evaluate: (fn: () => unknown) => parts.evaluate?.(fn) ?? Promise.resolve(undefined),
    keyboard: {
      press: (key: string) => parts.keyboardPress?.(key) ?? Promise.resolve(),
      type: (text: string) => parts.keyboardType?.(text) ?? Promise.resolve(),
    },
  } as unknown as Page;
}

/** A create dialog answering control queries by role or selector. */
function dialogMock(hooks: {
  onRole?: (role: string, options?: unknown) => Locator;
  onLocator?: (selector: string) => Locator;
  onTestId?: (id: string) => Locator;
}): Locator {
  return node({
    getByRole: (role: string, options?: unknown) => hooks.onRole?.(role, options) ?? absent(),
    locator: (selector: string) => hooks.onLocator?.(selector) ?? absent(),
    getByTestId: (id: string) => hooks.onTestId?.(id) ?? absent(),
  });
}

/** Route page.getByRole("dialog") (the createDialog scope) to the mock. */
function pageWithDialog(
  dialog: Locator,
  extra: PageParts = {},
  onRole?: (role: string, options?: unknown) => Locator
): Page {
  return pageOf({
    ...extra,
    role: (role: string, options?: unknown) => {
      if (role === "dialog") return dialog;
      return onRole?.(role, options) ?? extra.role?.(role, options) ?? absent();
    },
  });
}

describe("ModalDriver", () => {
  it("toggles the create-more switch", async () => {
    let clicked = false;
    const dialog = dialogMock({
      onRole: (role: string, options?: unknown) => {
        const want = (options as { name?: string } | undefined)?.name;
        if (role === "switch" && want === "Create more") {
          return node({
            click: async () => {
              clicked = true;
            },
          });
        }
        return absent();
      },
    });
    await new WebNewDriver(pageWithDialog(dialog)).enableCreateMore();
    expect(clicked).toBe(true);
  });

  it("reads the git branch value and empty when the section hides it", async () => {
    const dialog = dialogMock({
      onLocator: (selector: string) =>
        selector === "#git_work_branch" ? node({ inputValue: async () => "feature/x" }) : absent(),
    });
    await expect(new WebNewDriver(pageWithDialog(dialog)).gitBranchValue()).resolves.toBe("feature/x");
    await expect(new WebNewDriver(pageWithDialog(dialogMock({}))).gitBranchValue()).resolves.toBe("");
  });

  it("reads the git branch error and empty when valid", async () => {
    const dialog = dialogMock({
      onLocator: (selector: string) =>
        selector === "#git_work_branch-error" ? node({ textContent: async () => "Use letters." }) : absent(),
    });
    await expect(new WebNewDriver(pageWithDialog(dialog)).gitBranchError()).resolves.toBe("Use letters.");
    await expect(new WebNewDriver(pageWithDialog(dialogMock({}))).gitBranchError()).resolves.toBe("");
  });

  it("fills the branch through select-all plus typing", async () => {
    const calls: string[] = [];
    const dialog = dialogMock({
      onLocator: (selector: string) =>
        selector === "#git_work_branch"
          ? node({
              click: async () => {
                calls.push("click");
              },
              press: async (key: string) => {
                calls.push(`press:${key}`);
              },
              pressSequentially: async (text: string) => {
                calls.push(`type:${text}`);
              },
            })
          : absent(),
    });
    await new WebNewDriver(pageWithDialog(dialog)).fillGitBranch("feature/y");
    expect(calls).toEqual(["click", "press:ControlOrMeta+a", "press:Backspace", "type:feature/y"]);
  });

  it("toggles the advanced git section and waits for the field", async () => {
    const calls: string[] = [];
    const dialog = dialogMock({
      onRole: (role: string, options?: unknown) => {
        const want = (options as { name?: string } | undefined)?.name;
        if (role === "button" && want === "Advanced — git") {
          return node({
            click: async () => {
              calls.push("click:advanced");
            },
          });
        }
        return absent();
      },
      onLocator: (selector: string) =>
        selector === "#git_work_branch"
          ? node({
              waitFor: async () => {
                calls.push("wait:field");
              },
            })
          : absent(),
    });
    await new WebNewDriver(pageWithDialog(dialog)).toggleAdvancedGit();
    expect(calls).toEqual(["click:advanced", "wait:field"]);
  });

  it("opens a row menu entry by right-click", async () => {
    const calls: string[] = [];
    const page = pageOf({
      role: (role: string, options?: unknown) => {
        if (role === "region") {
          return node({
            locator: (selector: string) => {
              calls.push(`scope:${selector}`);
              return node({
                filter: () =>
                  node({
                    click: async (opts?: unknown) => {
                      calls.push(`click:${String((opts as { button?: string } | undefined)?.button ?? "left")}`);
                    },
                  }),
              });
            },
          });
        }
        if (role === "menuitem") {
          const want = (options as { name?: string } | undefined)?.name;
          return node({
            click: async () => {
              calls.push(`pick:${String(want)}`);
            },
          });
        }
        return absent();
      },
    });
    await new WebNewDriver(page).openRowMenuEntry("Target", "Archive");
    expect(calls).toContain('scope:a[data-row-anchor="true"]');
    expect(calls).toContain("click:right");
    expect(calls).toContain("pick:Archive");
  });

  it("reads a modal button's disabled state", async () => {
    const dialog = dialogMock({
      onRole: () => node({ isDisabled: async () => true }),
    });
    await expect(new WebNewDriver(pageWithDialog(dialog)).modalButtonDisabled("Create")).resolves.toBe(true);
  });

  it("opens the modal parent search and fills it", async () => {
    const calls: string[] = [];
    const dialog = dialogMock({
      onRole: (role: string, options?: unknown) => {
        const want = (options as { name?: string } | undefined)?.name;
        if (role === "button" && want === "Add parent") {
          return node({
            click: async () => {
              calls.push("click:add");
            },
          });
        }
        return absent();
      },
    });
    const page = pageWithDialog(dialog, {
      label: (label: string) =>
        label === "Search work items"
          ? node({
              fill: async (value: string) => {
                calls.push(`fill:${value}`);
              },
            })
          : absent(),
    });
    const driver = new WebNewDriver(page);
    await driver.openParentPicker();
    await driver.searchParentInModal("Parity");
    expect(calls).toEqual(["click:add", "fill:Parity"]);
  });

  it("picks a parent candidate and settles on the tag", async () => {
    const calls: string[] = [];
    const dialog = dialogMock({
      onTestId: (id: string) =>
        id === "parent-tag"
          ? node({
              waitFor: async () => {
                calls.push("wait:tag");
              },
            })
          : absent(),
    });
    const page = pageWithDialog(dialog, {
      locator: (selector: string) =>
        selector === "li"
          ? node({
              filter: () =>
                node({
                  getByRole: () =>
                    node({
                      click: async () => {
                        calls.push("click:candidate");
                      },
                    }),
                }),
            })
          : absent(),
    });
    await new WebNewDriver(page).selectParentResult("Parity parent");
    expect(calls).toEqual(["click:candidate", "wait:tag"]);
  });

  it("counts the candidate new-tab links", async () => {
    const page = pageOf({
      locator: (selector: string) => (selector === 'a[target="_blank"]' ? node({ count: async () => 3 }) : absent()),
    });
    await expect(new WebNewDriver(page).parentResultNewTabLinks()).resolves.toBe(3);
  });

  it("removes the modal parent through its tag", async () => {
    let clicked = false;
    const tag = node({
      getByRole: (inner: string) =>
        inner === "button"
          ? node({
              click: async () => {
                clicked = true;
              },
            })
          : absent(),
    });
    const dialog = dialogMock({
      onTestId: (id: string) =>
        id === "parent-tag"
          ? node({
              filter: () => tag,
            })
          : absent(),
    });
    await new WebNewDriver(pageWithDialog(dialog)).removeParentInModal("Parity parent");
    expect(clicked).toBe(true);
  });

  it("hovers the named row anchor", async () => {
    let hovered = false;
    const page = pageOf({
      role: (role: string) => {
        if (role !== "region") return absent();
        return node({
          locator: () =>
            node({
              filter: () =>
                node({
                  hover: async () => {
                    hovered = true;
                  },
                }),
            }),
        });
      },
    });
    await new WebNewDriver(page).hoverIssueRow("Target");
    expect(hovered).toBe(true);
  });

  it("expands sections through every load-more button", async () => {
    let clicks = 0;
    const page = pageOf({
      role: (role: string, options?: unknown) => {
        const want = (options as { name?: string } | undefined)?.name;
        if (role === "button" && want === "Load more") {
          return node({
            count: async () => 2,
            nth: () =>
              node({
                click: async () => {
                  clicks += 1;
                },
              }),
          });
        }
        return absent();
      },
    });
    await expect(new WebNewDriver(page).expandListRows()).resolves.toBe(true);
    expect(clicks).toBe(2);
    await expect(new WebNewDriver(pageOf()).expandListRows()).resolves.toBe(false);
  });

  it("reads the modal tab order from the dialog", async () => {
    const dialog = node({
      evaluate: async () => ["input#1:Title", "textarea#2:Description"],
    });
    await expect(new WebNewDriver(pageWithDialog(dialog)).modalTabOrder()).resolves.toEqual([
      "input#1:Title",
      "textarea#2:Description",
    ]);
  });

  it("opens the module page and settles on the header", async () => {
    const calls: string[] = [];
    const page = pageOf({
      goto: async (url: string) => {
        calls.push(`goto:${url}`);
      },
      testId: (id: string) => (id === "issues-header" ? node({}) : absent()),
    });
    await new WebNewDriver(page).openModulePage("ws", "pid", "mid");
    expect(calls).toEqual(["goto:/ws/projects/pid/modules/mid"]);
  });

  it("opens a row menu entry from the fresh strip past the region", async () => {
    const calls: string[] = [];
    const page = pageOf({
      role: (role: string, options?: unknown) => {
        if (role === "region") {
          return node({
            locator: () =>
              node({
                filter: () => absent(),
              }),
          });
        }
        if (role === "menuitem") {
          const want = (options as { name?: string } | undefined)?.name;
          return node({
            click: async () => {
              calls.push(`pick:${String(want)}`);
            },
          });
        }
        return absent();
      },
      locator: (selector: string) =>
        selector === 'a[data-row-anchor="true"]'
          ? node({
              filter: () =>
                node({
                  click: async (opts?: unknown) => {
                    calls.push(`click:${String((opts as { button?: string } | undefined)?.button ?? "left")}`);
                  },
                }),
            })
          : absent(),
    });
    await new WebNewDriver(page).openRowMenuEntry("Fresh", "Edit");
    expect(calls).toContain("click:right");
    expect(calls).toContain("pick:Edit");
  });

  it("hovers a fresh-strip row past the region", async () => {
    let hovered = false;
    const page = pageOf({
      role: () => absent(),
      locator: (selector: string) =>
        selector === 'a[data-row-anchor="true"]'
          ? node({
              filter: () =>
                node({
                  hover: async () => {
                    hovered = true;
                  },
                }),
            })
          : absent(),
    });
    await new WebNewDriver(page).hoverIssueRow("Fresh");
    expect(hovered).toBe(true);
  });

  it("reads the modal project name without the mark initial", async () => {
    const chip = node({
      textContent: async () => "PParity Project",
      locator: (selector: string) =>
        selector === "span.truncate" ? node({ textContent: async () => "Parity Project" }) : absent(),
    });
    const dialog = dialogMock({
      onLocator: (selector: string) =>
        selector === "form" ? node({ getByRole: (role: string) => (role === "button" ? chip : absent()) }) : absent(),
    });
    await expect(new WebNewDriver(pageWithDialog(dialog)).modalProjectName()).resolves.toBe("Parity Project");
  });

  it("falls back to the chip text without a name span", async () => {
    const chip = node({ textContent: async () => "Parity Project" });
    const dialog = dialogMock({
      onLocator: (selector: string) =>
        selector === "form" ? node({ getByRole: (role: string) => (role === "button" ? chip : absent()) }) : absent(),
    });
    await expect(new WebNewDriver(pageWithDialog(dialog)).modalProjectName()).resolves.toBe("Parity Project");
  });
});

describe("IssueModalProjectPicker", () => {
  const LABEL_SPANS = ':scope > span:not([aria-hidden="true"])';

  function chipDialog(chip: Locator): Locator {
    return dialogMock({
      onLocator: (selector: string) =>
        selector === "form" ? node({ getByRole: (role: string) => (role === "button" ? chip : absent()) }) : absent(),
    });
  }

  function optionList(options: Locator): (role: string) => Locator {
    return (role: string) => (role === "listbox" ? options : absent());
  }

  it("reads the project value through the chip name", async () => {
    const chip = node({
      textContent: async () => "PParity Project",
      locator: (selector: string) =>
        selector === "span.truncate" ? node({ textContent: async () => "Parity Project" }) : absent(),
    });
    await expect(new WebNewDriver(pageWithDialog(chipDialog(chip))).issueModalProjectValue()).resolves.toBe(
      "Parity Project"
    );
  });

  it("opens the picker on the chip and settles on options", async () => {
    const calls: string[] = [];
    const chip = node({
      click: async () => {
        calls.push("chip");
      },
    });
    const options = node({
      getByRole: (role: string) =>
        role === "option"
          ? node({
              waitFor: async () => {
                calls.push("settled");
              },
            })
          : absent(),
    });
    const page = pageWithDialog(chipDialog(chip), {}, optionList(options));
    await new WebNewDriver(page).issueModalProjectOpenPicker();
    expect(calls).toEqual(["chip", "settled"]);
  });

  it("skips the chip click when the search box is already mounted", async () => {
    let clicked = false;
    const chip = node({
      click: async () => {
        clicked = true;
      },
    });
    const page = pageWithDialog(chipDialog(chip), {}, (role: string) =>
      role === "combobox" ? node() : role === "listbox" ? node() : absent()
    );
    await new WebNewDriver(page).issueModalProjectOpenPicker();
    expect(clicked).toBe(false);
  });

  it("reads option labels from the label spans only", async () => {
    const options = node({
      getByRole: (role: string) =>
        role === "option"
          ? node({
              locator: (selector: string) =>
                selector === LABEL_SPANS ? node({ allInnerTexts: async () => [" Parity ", "B", ""] }) : absent(),
            })
          : absent(),
    });
    const page = pageWithDialog(dialogMock({}), {}, optionList(options));
    await expect(new WebNewDriver(page).issueModalProjectOptionTexts()).resolves.toEqual(["Parity", "B"]);
  });

  it("types the search query into the picker search box", async () => {
    let typed = "";
    const page = pageWithDialog(dialogMock({}), {}, (role: string) =>
      role === "combobox" ? node({ pressSequentially: async (text: string) => void (typed = text) }) : absent()
    );
    await new WebNewDriver(page).issueModalProjectSearch("Bravo");
    expect(typed).toBe("Bravo");
  });

  it("reads the empty line only when no options render", async () => {
    const withOptions = node({ getByRole: () => node({ count: async () => 2 }) });
    const withOptionsPage = pageWithDialog(dialogMock({}), {}, optionList(withOptions));
    await expect(new WebNewDriver(withOptionsPage).issueModalProjectEmptyText()).resolves.toBe("");

    const empty = node({
      getByRole: () => node({ count: async () => 0 }),
    });
    const emptyPage = pageWithDialog(dialogMock({}), {}, (role: string) => {
      if (role === "listbox") return empty;
      if (role === "status") return node({ innerText: async () => "No matching results" });
      return absent();
    });
    await expect(new WebNewDriver(emptyPage).issueModalProjectEmptyText()).resolves.toBe("No matching results");

    const barePage = pageWithDialog(dialogMock({}), {}, (role: string) => (role === "listbox" ? empty : absent()));
    await expect(new WebNewDriver(barePage).issueModalProjectEmptyText()).resolves.toBe("");
  });

  it("picks the option naming the project", async () => {
    let picked = "";
    const options = node({
      getByRole: (role: string, roleOptions?: unknown) => {
        if (role !== "option") return absent();
        const want = (roleOptions as { name?: string } | undefined)?.name ?? "";
        return node({
          click: async () => {
            picked = want;
          },
        });
      },
    });
    const page = pageWithDialog(dialogMock({}), {}, optionList(options));
    await new WebNewDriver(page).issueModalProjectPick("Bravo");
    expect(picked).toBe("Bravo");
  });

  it("presses Escape once when the popup closes at once", async () => {
    const presses: string[] = [];
    const options = node({ waitFor: async () => undefined });
    const page = pageWithDialog(
      dialogMock({}),
      { keyboardPress: async (key: string) => void presses.push(key) },
      (role: string) => (role === "listbox" ? options : absent())
    );
    await new WebNewDriver(page).issueModalProjectPressEscape();
    expect(presses).toEqual(["Escape"]);
  });

  it("presses Escape again when the first only clears the query", async () => {
    const presses: string[] = [];
    let waits = 0;
    const options = node({
      waitFor: async () => {
        waits += 1;
        if (waits === 1) throw new Error("still open");
      },
    });
    const page = pageWithDialog(
      dialogMock({}),
      { keyboardPress: async (key: string) => void presses.push(key) },
      (role: string) => (role === "listbox" ? options : absent())
    );
    await new WebNewDriver(page).issueModalProjectPressEscape();
    expect(presses).toEqual(["Escape", "Escape"]);
  });

  it("fills the title through real keystrokes", async () => {
    const calls: string[] = [];
    const field = node({
      click: async () => {
        calls.push("focus");
      },
      inputValue: async () => "Hello",
    });
    const dialog = node({
      getByPlaceholder: (text: string) => (text === "Title" ? field : absent()),
    });
    const page = pageWithDialog(
      dialog,
      {
        keyboardPress: async (key: string) => void calls.push(`press:${key}`),
        keyboardType: async (text: string) => void calls.push(`type:${text}`),
      },
      () => absent()
    );
    await new WebNewDriver(page).issueModalFillTitle("Hello");
    expect(calls).toEqual(["focus", "press:ControlOrMeta+a", "press:Backspace", "type:Hello"]);
  });

  it("submits and waits for the dialog to detach", async () => {
    const calls: string[] = [];
    const dialog = node({
      locator: (selector: string) =>
        selector === 'button[type="submit"]'
          ? node({
              click: async () => {
                calls.push("submit");
              },
            })
          : absent(),
      waitFor: async (options?: unknown) => {
        calls.push(`wait:${JSON.stringify(options)}`);
      },
    });
    await new WebNewDriver(pageWithDialog(dialog)).issueModalSubmit();
    expect(calls).toEqual(["submit", `wait:${JSON.stringify({ state: "detached", timeout: 60_000 })}`]);
  });
});
