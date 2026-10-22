// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver estimates-entry and repository-tab
// methods (NEWFRONT-602, PSET-049–052). A stub Page stands in for
// Playwright: the assertions cover the address each method opens, the
// selectors it reads, the route stub it installs and the settled state it
// reports — never a real browser.
import type { Locator, Page, Route } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  isChecked?: () => Promise<boolean>;
  isDisabled?: () => Promise<boolean>;
  click?: (options?: unknown) => Promise<void>;
  getAttribute?: (name: string) => Promise<string | null>;
  innerText?: () => Promise<string>;
  allInnerTexts?: () => Promise<string[]>;
  waitFor?: (options?: unknown) => Promise<unknown>;
  first?: () => StubLocator;
  last?: () => StubLocator;
  filter?: (options: unknown) => StubLocator;
  getByRole?: (role: string, options?: unknown) => StubLocator;
  getByText?: (text: string | RegExp, options?: unknown) => StubLocator;
  locator?: (selector: string) => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator & StubLocator {
  const node: StubLocator = {
    count: async () => 0,
    isVisible: async () => false,
    isChecked: async () => false,
    isDisabled: async () => false,
    click: async () => undefined,
    innerText: async () => "",
    allInnerTexts: async () => [],
    waitFor: async () => undefined,
    ...overrides,
  };
  const defaults: StubLocator = {
    first: () => node,
    last: () => node,
    filter: () => node,
    getByRole: () => node,
    getByText: () => node,
    locator: () => node,
  };
  for (const [name, value] of Object.entries(defaults)) {
    if (node[name] === undefined) node[name] = value;
  }
  return node as unknown as Locator & StubLocator;
}

function pageOf(overrides: Partial<Record<string, unknown>> = {}): Page {
  return {
    goto: vi.fn(async () => null),
    waitForLoadState: vi.fn(async () => undefined),
    getByRole: vi.fn(() => locator()),
    getByText: vi.fn(() => locator()),
    locator: vi.fn(() => locator()),
    route: vi.fn(async () => undefined),
    waitForResponse: vi.fn(async () => ({})),
    waitForTimeout: vi.fn(async () => undefined),
    ...overrides,
  } as unknown as Page;
}

function stubRoute(method: string) {
  return {
    request: () => ({ method: () => method }),
    continue: vi.fn(async () => undefined),
    abort: vi.fn(async () => undefined),
  } as unknown as Route & { continue: ReturnType<typeof vi.fn>; abort: ReturnType<typeof vi.fn> };
}

describe("estimates entry driver", () => {
  it("opens the estimates address and waits for the settings heading", async () => {
    const goto = vi.fn(async () => null);
    const waitFor = vi.fn(async () => undefined);
    const page = pageOf({
      goto,
      getByRole: () => locator({ first: () => locator({ waitFor }) }),
    });
    await new WebNewDriver(page).estimatesOpenSettings("acme", "p1");
    expect(goto).toHaveBeenCalledWith("/acme/settings/projects/p1/estimates");
    expect(waitFor).toHaveBeenCalledWith({ timeout: 60_000 });
  });

  it("retries once and then fails when neither the screen nor the denial paints", async () => {
    const goto = vi.fn(async () => null);
    const waitFor = vi.fn(async () => {
      throw new Error("timeout");
    });
    const page = pageOf({ goto, getByRole: () => locator({ first: () => locator({ waitFor }) }) });
    await expect(new WebNewDriver(page).estimatesOpenSettings("acme", "p1")).rejects.toThrow(
      "estimates settings settled on neither"
    );
    expect(goto).toHaveBeenCalledTimes(2);
  });

  it("reads the denial heading and the screen heading", async () => {
    const page = pageOf({
      getByRole: (role: string, options?: { name?: unknown }) =>
        role === "heading" && options?.name instanceof RegExp
          ? locator({ count: async () => 1 })
          : locator({ count: async () => 1, isVisible: async () => true }),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.estimatesNotAuthorized()).toBe(true);
    expect(await driver.estimatesHeadingVisible()).toBe(true);
  });

  it("reads the settings page title as the breadcrumb only while the screen mounts", async () => {
    const screen = locator({ count: async () => 1, isVisible: async () => true });
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="estimates-screen"]'
          ? screen
          : locator({ count: async () => 1, innerText: async () => "  Estimates " }),
    });
    expect(await new WebNewDriver(page).estimatesBreadcrumbText()).toBe("Estimates");
    const absent = pageOf();
    expect(await new WebNewDriver(absent).estimatesBreadcrumbText()).toBe("");
  });

  it("reports the loader only when its four skeletons show", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[role="status"][aria-label="Loading estimates"]'
          ? locator({
              count: async () => 1,
              isVisible: async () => true,
              locator: () => locator({ count: async () => 4 }),
            })
          : locator(),
    });
    expect(await new WebNewDriver(page).estimatesSkeletonVisible()).toBe(true);
    const short = pageOf({
      locator: (selector: string) =>
        selector === '[role="status"][aria-label="Loading estimates"]'
          ? locator({
              count: async () => 1,
              isVisible: async () => true,
              locator: () => locator({ count: async () => 2 }),
            })
          : locator(),
    });
    expect(await new WebNewDriver(short).estimatesSkeletonVisible()).toBe(false);
  });

  it("holds list reads but passes writes through", async () => {
    const route = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route }));
    await driver.estimatesDelayListFetch(0);
    const pattern = route.mock.calls[0]?.[0];
    expect(pattern).toBe("**/api/workspaces/*/projects/*/estimates/");
    const handler = route.mock.calls[0]?.[1] as (route: Route) => Promise<void>;
    const write = stubRoute("POST");
    await handler(write);
    expect(write.continue).toHaveBeenCalled();
    const read = stubRoute("GET");
    await handler(read);
    expect(read.continue).toHaveBeenCalled();
  });

  it("reads the empty title and clicks the empty-state action", async () => {
    const click = vi.fn(async () => undefined);
    const page = pageOf({
      getByRole: () => locator({ count: async () => 1, isVisible: async () => true, click }),
      locator: () => locator(),
    });
    page.locator = vi.fn(() => locator({ count: async () => 1, innerText: async () => " No estimates yet " }));
    page.getByText = vi.fn(() => locator({ count: async () => 1, innerText: async () => " No estimates yet " }));
    const driver = new WebNewDriver(page);
    expect(await driver.estimatesEmptyVisible()).toBe(true);
    expect(await driver.estimatesEmptyTitle()).toBe("No estimates yet");
    expect(await new WebNewDriver(pageOf()).estimatesEmptyTitle()).toBe("");
    const short = pageOf({
      getByText: () =>
        locator({
          count: async () => 1,
          innerText: async () => {
            throw new Error("detached");
          },
        }),
    });
    expect(await new WebNewDriver(short).estimatesEmptyTitle()).toBe("");
    await driver.estimatesEmptyAddClick();
    expect(click).toHaveBeenCalled();
  });

  it("reports the creation dialog from the app dialog carrying its title", async () => {
    const page = pageOf({
      getByRole: () =>
        locator({ count: async () => 1, filter: () => locator({ count: async () => 1, isVisible: async () => true }) }),
    });
    expect(await new WebNewDriver(page).estimatesCreateDialogOpen()).toBe(true);
    expect(await new WebNewDriver(pageOf()).estimatesCreateDialogOpen()).toBe(false);
  });
});

describe("estimates active, switch and archived driver", () => {
  it("reads the active row name, values and its single control", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="estimates-active-section"] [data-testid="estimate-system-row"]'
          ? locator({
              count: async () => 1,
              isVisible: async () => true,
              getByRole: () => locator({ count: async () => 0 }),
              locator: (inner: string) =>
                inner === "h4"
                  ? locator({ innerText: async () => " Points " })
                  : locator({ innerText: async () => " 1, 2, 3 " }),
            })
          : locator(),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.estimatesActiveSystemName()).toBe("Points");
    expect(await driver.estimatesEditDeleteActiveRowEditControlVisible()).toBe(false);

    const unreadable = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="estimates-active-section"] [data-testid="estimate-system-row"]'
          ? locator({
              count: async () => 1,
              isVisible: async () => true,
              locator: () =>
                locator({
                  innerText: async () => {
                    throw new Error("detached");
                  },
                }),
            })
          : locator(),
    });
    expect(await new WebNewDriver(unreadable).estimatesActiveSystemName()).toBe("");
  });

  it("reads the active row's value line and treats a missing row as empty", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="estimates-active-section"] [data-testid="estimate-system-row"]'
          ? locator({
              count: async () => 1,
              isVisible: async () => true,
              locator: () => locator({ innerText: async () => "  1, 2, 3  " }),
            })
          : locator(),
    });
    expect(await new WebNewDriver(page).estimatesActiveSystemValues()).toBe("1, 2, 3");

    const unreadable = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="estimates-active-section"] [data-testid="estimate-system-row"]'
          ? locator({
              count: async () => 1,
              isVisible: async () => true,
              locator: () =>
                locator({
                  innerText: async () => {
                    throw new Error("detached");
                  },
                }),
            })
          : locator(),
    });
    expect(await new WebNewDriver(unreadable).estimatesActiveSystemValues()).toBe("");
    expect(await new WebNewDriver(pageOf()).estimatesActiveSystemValues()).toBe("");
  });

  it("counts the active row's controls and none when the row is absent", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="estimates-active-section"] [data-testid="estimate-system-row"]'
          ? locator({ count: async () => 1, getByRole: () => locator({ count: async () => 1 }) })
          : locator(),
    });
    expect(await new WebNewDriver(page).estimatesActiveRowButtonCount()).toBe(1);
    expect(await new WebNewDriver(pageOf()).estimatesActiveRowButtonCount()).toBe(0);
  });

  it("opens the create dialog and detects its step two", async () => {
    const click = vi.fn(async () => undefined);
    const page = pageOf({
      getByRole: () =>
        locator({
          first: () => locator({ click }),
          filter: () => locator({ count: async () => 1, isVisible: async () => true }),
        }),
      locator: () =>
        locator({ count: async () => 1, isVisible: async () => true, innerText: async () => "Step 2 of 2" }),
    });
    const driver = new WebNewDriver(page);
    await driver.estimatesOpenCreateDialog();
    expect(click).toHaveBeenCalled();
    await driver.estimatesPickStarter("Custom");
    expect(await driver.estimatesDialogOpen()).toBe(true);
  });

  it("submits through the dialog's create action", async () => {
    const click = vi.fn(async () => undefined);
    const page = pageOf({
      getByRole: () =>
        locator({
          filter: () =>
            locator({ count: async () => 0, getByRole: () => locator({ first: () => locator({ click }) }) }),
        }),
    });
    await new WebNewDriver(page).estimatesSubmit();
    expect(click).toHaveBeenCalled();
  });

  it("opens the delete confirm and cancels it", async () => {
    const click = vi.fn(async () => undefined);
    const page = pageOf({
      getByRole: () => locator({ first: () => locator({ click }) }),
      locator: (selector: string) =>
        selector.includes("estimate-system-row")
          ? locator({ getByRole: () => locator({ first: () => locator({ click }) }) })
          : locator(),
    });
    const driver = new WebNewDriver(page);
    await driver.estimatesActiveRowDeleteClick();
    expect(click).toHaveBeenCalled();
    await driver.estimatesConfirmDialogCancel();
    await driver.estimatesEditDeleteConfirmDialogConfirm();
  });

  it("reads the confirm dialog's title and body", async () => {
    const page = pageOf({
      locator: () => locator({ count: async () => 1, innerText: async () => "Delete estimate system\nSome body" }),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.estimatesConfirmDialogOpen()).toBe(true);
    expect(await driver.estimatesEditDeleteConfirmDialogTitle()).toBe("Delete estimate system");
    expect(await driver.estimatesEditDeleteConfirmDialogBody()).toBe("Delete estimate system Some body");
    const blank = new WebNewDriver(pageOf());
    expect(await blank.estimatesEditDeleteConfirmDialogTitle()).toBe("");
    expect(await blank.estimatesEditDeleteConfirmDialogBody()).toBe("");
  });

  it("reads the switch's state and toggles it", async () => {
    const click = vi.fn(async () => undefined);
    const page = pageOf({
      getByRole: (role: string) =>
        role === "switch"
          ? locator({
              count: async () => 1,
              isVisible: async () => true,
              isChecked: async () => true,
              isDisabled: async () => false,
              click,
            })
          : locator({ first: () => locator({ waitFor: async () => undefined }) }),
      waitForResponse: vi.fn(async () => ({})),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.estimatesEnableSwitchVisible()).toBe(true);
    expect(await driver.estimatesEnableSwitchOn()).toBe(true);
    expect(await driver.estimatesEnableSwitchDisabled()).toBe(false);
    await driver.estimatesEnableSwitchToggle();
    expect(click).toHaveBeenCalled();
  });

  it("settles the enable toggle on the project write or its notice", async () => {
    const waitForResponse = vi.fn(async () => ({}));
    const page = pageOf({
      getByRole: (role: string) =>
        role === "switch" ? locator({ count: async () => 1, isVisible: async () => true }) : locator(),
      waitForResponse,
    });
    await new WebNewDriver(page).estimatesEnableSwitchToggle();
    const predicate = waitForResponse.mock.calls[0]?.[0] as unknown as (response: {
      url: () => string;
      request: () => { method: () => string };
    }) => boolean;
    const answer = (url: string, method: string) =>
      predicate({ url: () => url, request: () => ({ method: () => method }) });
    expect(answer("https://app.test/api/workspaces/acme/projects/p1/", "PATCH")).toBe(true);
    expect(answer("https://app.test/api/workspaces/acme/projects/p1/", "PUT")).toBe(true);
    expect(answer("https://app.test/api/workspaces/acme/projects/p1/", "GET")).toBe(false);
    expect(answer("https://app.test/api/workspaces/acme/projects/p1/estimates/", "PATCH")).toBe(false);
  });

  it("refuses project writes while letting reads through", async () => {
    const route = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route }));
    await driver.estimatesFailProjectUpdate();
    const handler = route.mock.calls[0]?.[1] as (route: Route) => Promise<void>;
    const read = stubRoute("GET");
    await handler(read);
    expect(read.continue).toHaveBeenCalled();
    const write = stubRoute("PATCH");
    await handler(write);
    expect(write.abort).toHaveBeenCalledWith("failed");
  });

  it("lists archived systems read-only", async () => {
    const reference = locator({
      count: async () => 1,
      getAttribute: async (name: string) =>
        name === "href" ? "https://example.test/estimates" : name === "target" ? "_blank" : null,
    });
    const page = pageOf({
      locator: (selector: string) => {
        if (selector === '[data-testid="estimates-archived-section"]') {
          return locator({
            count: async () => 1,
            isVisible: async () => true,
            locator: (inner: string) => {
              if (inner.endsWith("h4")) return locator({ allInnerTexts: async () => [" B ", "A"] });
              if (inner === "h3 + p") return locator({ count: async () => 1, innerText: async () => " Old systems " });
              if (inner === '[data-testid="estimates-archived-reference"]') return reference;
              if (inner.includes("estimate-system-row"))
                return locator({ getByRole: () => locator({ count: async () => 0 }) });
              return locator();
            },
          });
        }
        return locator();
      },
    });
    const driver = new WebNewDriver(page);
    expect(await driver.estimatesArchivedSectionVisible()).toBe(true);
    expect(await driver.estimatesArchivedNames()).toEqual(["B", "A"]);
    expect(await driver.estimatesArchivedExplainerText()).toBe("Old systems");
    expect(await driver.estimatesArchivedRowButtonCount("A")).toBe(0);
    expect(await driver.estimatesArchivedLinkHref()).toBe("https://example.test/estimates");
    expect(await new WebNewDriver(pageOf()).estimatesArchivedNames()).toEqual([]);
    expect(await new WebNewDriver(pageOf()).estimatesArchivedExplainerText()).toBe("");
    expect(await driver.estimatesArchivedLinkTarget()).toBe("_blank");
    expect(await new WebNewDriver(pageOf()).estimatesArchivedLinkHref()).toBe("");
    expect(await new WebNewDriver(pageOf()).estimatesArchivedLinkTarget()).toBe("");
  });

  it("counts an archived row's controls, or none when the section or the row is absent", async () => {
    expect(await new WebNewDriver(pageOf()).estimatesArchivedRowButtonCount("A")).toBe(0);
    const noControls = locator({ count: async () => 0 });
    const row = locator({ count: async () => 1, getByRole: () => noControls });
    const present = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="estimates-archived-section"]'
          ? locator({
              count: async () => 1,
              isVisible: async () => true,
              locator: () => locator({ filter: () => locator({ first: () => row }) }),
            })
          : locator(),
    });
    expect(await new WebNewDriver(present).estimatesArchivedRowButtonCount("A")).toBe(0);
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="estimates-archived-section"]'
          ? locator({
              count: async () => 1,
              isVisible: async () => true,
              locator: () => locator({ filter: () => locator({ first: () => locator({ count: async () => 0 }) }) }),
            })
          : locator(),
    });
    expect(await new WebNewDriver(page).estimatesArchivedRowButtonCount("A")).toBe(0);
  });

  it("reads a blank or unreadable link attribute as absent", async () => {
    const missingTarget = locator({
      count: async () => 1,
      getAttribute: async (name: string) => {
        if (name === "href") throw new Error("detached");
        return null;
      },
    });
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="estimates-archived-section"]'
          ? locator({
              count: async () => 1,
              isVisible: async () => true,
              locator: (inner: string) =>
                inner === '[data-testid="estimates-archived-reference"]' ? missingTarget : locator(),
            })
          : locator(),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.estimatesArchivedLinkHref()).toBe("");
    expect(await driver.estimatesArchivedLinkTarget()).toBe("");
  });

  it("clicks the archived reference and resolves the page it opens", async () => {
    const click = vi.fn(async () => undefined);
    const close = vi.fn(async () => undefined);
    const waitForEvent = vi.fn(async () => ({ url: () => "https://example.test/estimates", close }));
    const page = pageOf({
      context: () => ({ waitForEvent }),
      locator: (selector: string) =>
        selector === '[data-testid="estimates-archived-section"]'
          ? locator({
              count: async () => 1,
              isVisible: async () => true,
              locator: (inner: string) =>
                inner === '[data-testid="estimates-archived-reference"]'
                  ? locator({ count: async () => 1, click })
                  : locator(),
            })
          : locator(),
    });
    await expect(new WebNewDriver(page).estimatesArchivedOpenLinkNewPage()).resolves.toBe(
      "https://example.test/estimates"
    );
    expect(waitForEvent).toHaveBeenCalledWith("page", { timeout: 30_000 });
    expect(click).toHaveBeenCalled();
    expect(close).toHaveBeenCalled();
  });
});

describe("repository tab driver", () => {
  it("opens the github address and settles on the bound card", async () => {
    const goto = vi.fn(async () => null);
    const waitFor = vi.fn(async () => undefined);
    const page = pageOf({
      goto,
      locator: () => locator({ waitFor, first: () => locator({ waitFor }) }),
    });
    await new WebNewDriver(page).projectGithubOpenTab("acme", "p1");
    expect(goto).toHaveBeenCalledWith("/acme/settings/projects/p1/github");
    expect(waitFor).toHaveBeenCalledWith({ timeout: 60_000 });
  });

  it("fails after two rounds when no card settles", async () => {
    const goto = vi.fn(async () => null);
    const page = pageOf({
      goto,
      locator: () =>
        locator({
          waitFor: async () => {
            throw new Error("timeout");
          },
        }),
      getByRole: () =>
        locator({
          first: () =>
            locator({
              waitFor: async () => {
                throw new Error("timeout");
              },
            }),
        }),
    });
    await expect(new WebNewDriver(page).projectGithubOpenTab("acme", "p1")).rejects.toThrow(
      "github tab settled on neither"
    );
    expect(goto).toHaveBeenCalledTimes(2);
  });

  it("reads the loader and holds binding reads", async () => {
    const route = vi.fn(async () => undefined);
    const driver = new WebNewDriver(pageOf({ route }));
    await driver.projectGithubLoaderVisible();
    await driver.projectGithubDelayBindingFetch(0);
    const handler = route.mock.calls[0]?.[1] as (route: Route) => Promise<void>;
    const read = stubRoute("GET");
    await handler(read);
    expect(read.continue).toHaveBeenCalled();
    const write = stubRoute("PATCH");
    await handler(write);
    expect(write.continue).toHaveBeenCalled();
  });

  it("reads the bound card's provider, link, contract, cadence and error", async () => {
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="repository-bound-card"]'
          ? locator({
              count: async () => 1,
              isVisible: async () => true,
              innerText: async () => "Mirrored issues are read-only. Workflow fields stay editable.",
              getByRole: (role: string) =>
                role === "link"
                  ? locator({ count: async () => 1, innerText: async () => " parity/parity-seed " })
                  : locator({ count: async () => 1 }),
              getByText: (text: string | RegExp) =>
                locator({
                  count: async () => 1,
                  innerText: async () =>
                    String(text).includes("Last error")
                      ? "Last error: boom"
                      : "Sync issues every 4 hours — last sync Oct 1",
                }),
            })
          : locator(),
      getByRole: (role: string, options?: { name?: string }) =>
        role === "heading" && options?.name === "GitHub"
          ? locator({ count: async () => 1 })
          : locator({
              count: async () => 1,
              isVisible: async () => true,
              isChecked: async () => true,
              isDisabled: async () => true,
            }),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.projectGithubBoundVisible()).toBe(true);
    expect(await driver.projectGithubProviderLabel()).toBe("GitHub");
    expect(await driver.projectGithubRepoLinkText()).toBe("parity/parity-seed");
    expect(await driver.projectGithubContractVisible()).toBe(true);
    expect(await driver.projectGithubCadenceText()).toBe("Sync issues every 4 hours — last sync Oct 1");
    const cadence = pageOf({
      locator: () =>
        locator({
          count: async () => 1,
          isVisible: async () => true,
          getByText: () => locator({ count: async () => 1, innerText: async () => "   " }),
        }),
    });
    expect(await new WebNewDriver(cadence).projectGithubCadenceText()).toBeNull();
    const unreadable = pageOf({
      locator: () =>
        locator({
          count: async () => 1,
          isVisible: async () => true,
          getByText: () =>
            locator({
              count: async () => 1,
              innerText: async () => {
                throw new Error("detached");
              },
            }),
        }),
    });
    expect(await new WebNewDriver(unreadable).projectGithubCadenceText()).toBeNull();
    expect(await driver.projectGithubLastError()).toBe("Last error: boom");
    expect(await driver.projectGithubSyncOn()).toBe(true);
    expect(await driver.projectGithubSyncDisabled()).toBe(true);
  });

  it("reports no provider label when the card names none", async () => {
    expect(await new WebNewDriver(pageOf()).projectGithubProviderLabel()).toBeNull();
  });

  it("toggles sync and unbinds without a confirmation", async () => {
    const click = vi.fn(async () => undefined);
    const page = pageOf({
      getByRole: () => locator({ count: async () => 1, isVisible: async () => true, click }),
      waitForResponse: vi.fn(async () => ({})),
    });
    const driver = new WebNewDriver(page);
    await driver.projectGithubToggleSync();
    await driver.projectGithubUnbind();
    expect(click).toHaveBeenCalled();
  });

  it("settles sync and unbind on their own binding writes", async () => {
    const waitForResponse = vi.fn(async () => ({}));
    const page = pageOf({
      getByRole: () => locator({ count: async () => 1, isVisible: async () => true }),
      waitForResponse,
    });
    const driver = new WebNewDriver(page);
    await driver.projectGithubToggleSync();
    await driver.projectGithubUnbind();
    expect(waitForResponse).toHaveBeenCalledTimes(2);
    const [syncPredicate, unbindPredicate] = waitForResponse.mock.calls.map(
      (call) =>
        call[0] as unknown as (response: { url: () => string; request: () => { method: () => string } }) => boolean
    );
    const answer = (
      predicate: (response: { url: () => string; request: () => { method: () => string } }) => boolean,
      url: string,
      method: string
    ) => predicate({ url: () => url, request: () => ({ method: () => method }) });
    const binding = "https://app.test/api/workspaces/acme/projects/p1/repository/";
    expect(answer(syncPredicate!, binding, "PATCH")).toBe(true);
    expect(answer(syncPredicate!, binding, "PUT")).toBe(true);
    expect(answer(syncPredicate!, binding, "DELETE")).toBe(false);
    expect(answer(syncPredicate!, "https://app.test/api/workspaces/acme/projects/p1/", "PATCH")).toBe(false);
    expect(answer(unbindPredicate!, binding, "DELETE")).toBe(true);
    expect(answer(unbindPredicate!, binding, "PATCH")).toBe(false);
  });

  it("reads unbind visibility and reads the guidance card text", async () => {
    const page = pageOf({
      getByRole: () => locator({ count: async () => 1, isDisabled: async () => false, click: async () => undefined }),
      locator: (selector: string) =>
        selector === '[data-testid="repository-guidance-card"]'
          ? locator({
              count: async () => 1,
              isVisible: async () => true,
              getByRole: () => locator({ count: async () => 1 }),
            })
          : selector === "body"
            ? locator({ innerText: async () => "Repository sync  Guidance " })
            : locator(),
    });
    const driver = new WebNewDriver(page);
    expect(await driver.projectGithubUnbindVisible()).toBe(true);
    expect(await driver.projectGithubUnbindDisabled()).toBe(false);
    expect(await driver.projectGithubGuidanceVisible()).toBe(true);
    expect(await driver.projectGithubTabText()).toBe("Repository sync Guidance");
    expect(await new WebNewDriver(pageOf()).projectGithubGuidanceVisible()).toBe(false);
    expect(await new WebNewDriver(pageOf()).projectGithubNotAuthorized()).toBe(false);
  });
});

describe("estimates shell driver (EST-001-003, EST-026-028)", () => {
  it("reads the screen text normalized, or blank when unreadable", async () => {
    const page = pageOf({
      getByRole: () => locator({ innerText: async () => "  Estimates\n  list " }),
    });
    expect(await new WebNewDriver(page).estimatesScreenText()).toBe("Estimates list");
    const unreadable = pageOf({
      getByRole: () =>
        locator({
          innerText: async () => {
            throw new Error("detached");
          },
        }),
    });
    expect(await new WebNewDriver(unreadable).estimatesScreenText()).toBe("");
  });

  it("lists the system names from their rows, skipping blanks and dead reads", async () => {
    const rows = [
      locator({ innerText: async () => "  Alpha " }),
      locator({ innerText: async () => " " }),
      locator({
        innerText: async () => {
          throw new Error("detached");
        },
      }),
      locator({ innerText: async () => "Beta" }),
    ];
    const inner = vi.fn(() => locator());
    const headAt = (index: number) => {
      const head = rows[index];
      if (head === undefined) throw new Error("row out of range");
      return head;
    };
    const page = pageOf({
      locator: (selector: string) =>
        selector === '[data-testid="estimates-screen"]'
          ? locator({
              locator: (rowSelector: string) => {
                inner(rowSelector);
                return locator({ count: async () => rows.length, nth: headAt });
              },
            })
          : locator(),
    });
    expect(await new WebNewDriver(page).estimatesListedNames()).toEqual(["Alpha", "Beta"]);
    expect(inner).toHaveBeenCalledWith('[data-testid="estimate-system-row"] h4');
    expect(await new WebNewDriver(pageOf()).estimatesListedNames()).toEqual([]);
  });

  it("reads the browser tab title", async () => {
    const title = vi.fn(async () => "Website relaunch - Estimates");
    expect(await new WebNewDriver(pageOf({ title })).estimatesTabTitle()).toBe("Website relaunch - Estimates");
  });

  it("reports export/import controls from buttons or menu items", async () => {
    const seen: unknown[] = [];
    const shown = locator({ count: async () => 1, isVisible: async () => true });
    const hidden = locator({ count: async () => 0 });
    const mainWith = (visibleRole: string) =>
      locator({
        getByRole: (role: string, options?: unknown) => {
          seen.push([role, options]);
          return role === visibleRole ? shown : hidden;
        },
      });
    const page = pageOf({ getByRole: (role: string) => (role === "main" ? mainWith("button") : hidden) });
    expect(await new WebNewDriver(page).estimatesExportImportVisible()).toBe(true);
    const viaMenu = pageOf({
      getByRole: (role: string) => (role === "main" ? mainWith("menuitem") : hidden),
    });
    expect(await new WebNewDriver(viaMenu).estimatesExportImportVisible()).toBe(true);
    expect(await new WebNewDriver(pageOf()).estimatesExportImportVisible()).toBe(false);
    expect(seen).toContainEqual(["button", { name: /export|import|download/i }]);
    expect(seen).toContainEqual(["menuitem", { name: /export|import|download/i }]);
  });

  it("reports filter/search controls from every query", async () => {
    const shown = locator({ count: async () => 1, isVisible: async () => true });
    const hidden = locator({ count: async () => 0 });
    const fromRole = (role: string) =>
      pageOf({
        getByRole: (candidate: string) =>
          candidate === "main" ? locator({ getByRole: (inner: string) => (inner === role ? shown : hidden) }) : hidden,
      });
    expect(await new WebNewDriver(fromRole("searchbox")).estimatesFilterSearchVisible()).toBe(true);
    expect(await new WebNewDriver(fromRole("button")).estimatesFilterSearchVisible()).toBe(true);
    const fromSelector = (selector: string) =>
      pageOf({
        getByRole: (candidate: string) =>
          candidate === "main"
            ? locator({
                getByRole: () => hidden,
                locator: (inner: string) => (inner === selector ? shown : hidden),
              })
            : hidden,
      });
    expect(await new WebNewDriver(fromSelector('input[type="search"]')).estimatesFilterSearchVisible()).toBe(true);
    expect(await new WebNewDriver(fromSelector('input[placeholder*="earch" i]')).estimatesFilterSearchVisible()).toBe(
      true
    );
    expect(await new WebNewDriver(pageOf()).estimatesFilterSearchVisible()).toBe(false);
  });

  it("reports bulk controls from checkboxes or bulk buttons", async () => {
    const shown = locator({ count: async () => 1, isVisible: async () => true });
    const hidden = locator({ count: async () => 0 });
    const fromRole = (role: string) =>
      pageOf({
        getByRole: (candidate: string) =>
          candidate === "main" ? locator({ getByRole: (inner: string) => (inner === role ? shown : hidden) }) : hidden,
      });
    expect(await new WebNewDriver(fromRole("checkbox")).estimatesBulkVisible()).toBe(true);
    expect(await new WebNewDriver(fromRole("button")).estimatesBulkVisible()).toBe(true);
    expect(await new WebNewDriver(pageOf()).estimatesBulkVisible()).toBe(false);
  });

  it("opens a guessed system address and settles on not-found or the screen", async () => {
    const goto = vi.fn(async () => null);
    const waitForLoadState = vi.fn(async () => undefined);
    const waitFor = vi.fn(async () => undefined);
    const page = pageOf({
      goto,
      waitForLoadState,
      getByRole: () => locator({ first: () => locator({ waitFor }) }),
    });
    await new WebNewDriver(page).estimatesOpenSystemGuess("acme", "p1", "e9");
    expect(goto).toHaveBeenCalledWith("/acme/settings/projects/p1/estimates/e9");
    expect(waitForLoadState).toHaveBeenCalledWith("domcontentloaded");
    expect(waitFor).toHaveBeenCalledWith({ timeout: 60_000 });
  });

  it("fails the system guess when neither surface settles", async () => {
    const waitFor = vi.fn(async () => {
      throw new Error("timeout");
    });
    const page = pageOf({
      getByRole: () => locator({ first: () => locator({ waitFor }) }),
    });
    await expect(new WebNewDriver(page).estimatesOpenSystemGuess("acme", "p1", "e9")).rejects.toThrow(
      "system-guess address settled on neither"
    );
  });

  it("reads the not-found heading", async () => {
    const getByRole = vi.fn(() => locator({ count: async () => 1 }));
    expect(await new WebNewDriver(pageOf({ getByRole })).estimatesNotFoundVisible()).toBe(true);
    expect(getByRole).toHaveBeenCalledWith("heading", { name: "That page does not exist", exact: true });
    expect(await new WebNewDriver(pageOf()).estimatesNotFoundVisible()).toBe(false);
  });
});
