// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for slice F's modules driver hooks (NEWFRONT-798, MOD-003/009/
// 022–025): the header and zero-state creation entry points, the create and
// update dialog reads/writes, the pinned tab sequence, the delete
// confirmation and its refusal shaping, the detail menu, and the row facts.
// A Playwright-shaped fake answers from a path-keyed table so each case pins
// the hook a method reads or the control it activates; no browser runs.

import type { Locator, Page } from "@playwright/test";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const routerHarness = vi.hoisted(() => ({ navigate: vi.fn() }));
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useNavigate: () => routerHarness.navigate,
}));

import { createAppClient, resetAppClient } from "../../../src/core/api/client.js";
import { ModulesZeroState } from "../../../src/features/modules/components/ModulesZeroState.js";
import { ModuleDetailActions } from "../../../src/features/modules/components/dialogs/ModuleDetailActions.js";
import { ModuleSaveDialog } from "../../../src/features/modules/components/dialogs/ModuleSaveDialog.js";
import {
  jsonResponse,
  makeProjectRow,
  MODULES_MODULE_ID,
  MODULES_PROJECT_ID,
  MODULES_WS_ID,
} from "../../../src/features/modules/testing.js";
import {
  MODULE_SELECTED_OPTION_SELECTOR,
  moduleActionsTriggerSelector,
  moduleVisibleTexts,
  WebNewDriver,
} from "./web-new";

interface Answer {
  count?: number;
  visible?: boolean;
  texts?: string[];
  text?: string;
  value?: string;
  attrs?: Record<string, string | null>;
  disabled?: boolean;
  evaluated?: unknown;
}

type Table = Record<string, Answer>;
type RouteHandler = (route: FakeRoute) => Promise<void> | void;

interface FakeRoute {
  request(): { method(): string; postData(): string | null };
  continue(): Promise<void>;
  abort(): Promise<void>;
  fulfill(input: { status: number; contentType?: string; body?: string }): Promise<void>;
}

function keyFor(parent: string, kind: string, args: unknown[]): string {
  const rendered = args
    .map((arg) => (arg && typeof arg === "object" ? String((arg as { name?: string }).name ?? "") : String(arg)))
    .join(",");
  return `${parent} ${kind}(${rendered})`;
}

class FakePage {
  readonly clicks: string[] = [];
  readonly fills: string[] = [];
  readonly presses: string[] = [];
  readonly keys: string[] = [];
  readonly pageCalls: string[] = [];
  readonly routes: { matcher: unknown; handler: RouteHandler }[] = [];

  constructor(private readonly table: Table = {}) {}

  private answer(id: string): Answer & { visible: boolean; count: number } {
    const candidates = Object.entries(this.table)
      .filter(([key]) => id === key || id.startsWith(key))
      .sort(([a], [b]) => b.length - a.length);
    return { visible: true, count: 1, ...(candidates[0]?.[1] ?? {}) };
  }

  locator(id: string): Locator {
    const self = this;
    const node: Record<string, unknown> = {
      first: () => self.locator(id),
      last: () => self.locator(id),
      nth: (index: number) => self.locator(`${id} nth(${index})`),
      filter: (options: { has?: Locator; hasText?: string | RegExp }) =>
        self.locator(`${id} has(${String((options.has as never)?.["__id"] ?? "")},${String(options.hasText ?? "")})`),
      getByTestId: (testId: string) => self.locator(`${id} testid(${testId})`),
      getByRole: (role: string, options?: { name?: string }) =>
        self.locator(`${id} role(${role},${options?.name ?? ""})`),
      getByLabel: (label: string) => self.locator(`${id} label(${label})`),
      locator: (selector: string) => self.locator(`${id} css(${selector})`),
      count: async () => self.answer(id).count,
      isVisible: async () => self.answer(id).visible,
      isDisabled: async () => self.answer(id).disabled ?? false,
      innerText: async () => self.answer(id).text ?? "",
      allInnerTexts: async () => self.answer(id).texts ?? [],
      inputValue: async () => self.answer(id).value ?? "",
      getAttribute: async (name: string) => self.answer(id).attrs?.[name] ?? null,
      click: async () => {
        self.clicks.push(id);
      },
      fill: async (value: string) => {
        self.fills.push(`${id}=${value}`);
      },
      press: async (key: string) => {
        self.presses.push(`${id}:${key}`);
      },
      selectOption: async () => undefined,
      waitFor: async () => undefined,
      evaluate: async () => self.answer(id).evaluated ?? {},
      // Reads that pass a DOM function land here; the table may answer with
      // the list directly or with the plain text it used to return.
      evaluateAll: async () => {
        const answer = self.answer(id);
        if (answer.evaluated !== undefined) return answer.evaluated;
        if (answer.texts !== undefined) return answer.texts;
        return answer.text === undefined ? [] : [answer.text];
      },
    };
    Object.defineProperty(node, "__id", { value: id, enumerable: false });
    return node as unknown as Locator;
  }

  asPage(): Page {
    const self = this;
    const page = {
      getByTestId: (testId: string) => {
        self.keys.push(`testid:${testId}`);
        return self.locator(keyFor("page", "testid", [testId]));
      },
      getByRole: (role: string, options?: { name?: string }) => {
        self.keys.push(`role:${role}:${options?.name ?? ""}`);
        return self.locator(keyFor("page", "role", [role, options?.name ?? ""]));
      },
      locator: (selector: string) => {
        self.keys.push(`css:${selector}`);
        return self.locator(keyFor("page", "css", [selector]));
      },
      url: () => "https://app.test/acme/projects/p1/modules",
      title: async () => "Website relaunch - Modules",
      evaluate: async (callback: (arg?: unknown) => unknown) => callback(),
      goto: async (url: string) => {
        self.pageCalls.push(`goto:${url}`);
      },
      waitForLoadState: async () => {
        self.pageCalls.push("waitForLoadState");
      },
      waitForResponse: async () => null,
      waitForTimeout: async () => undefined,
      keyboard: {
        press: async (key: string) => {
          self.presses.push(`page:${key}`);
        },
      },
      route: async (matcher: unknown, handler: RouteHandler) => {
        self.routes.push({ matcher, handler });
      },
      unrouteAll: async () => {
        self.pageCalls.push("unrouteAll");
      },
      context: () => ({ addCookies: async () => undefined }),
    };
    return page as unknown as Page;
  }
}

function route(method: string, path: string, _handler: RouteHandler): FakeRoute {
  const events: string[] = [];
  return {
    request: () => ({ method: () => method, postData: () => null }),
    continue: async () => {
      events.push("continue");
      void path;
    },
    abort: async () => {
      events.push("abort");
    },
    fulfill: async (input) => {
      events.push(`fulfill:${input.status}`);
    },
  };
}

const SAVE = 'page role(dialog,) has(page css(input[name="name"]),)';

function saveDialogTable(extra: Table = {}): Table {
  return {
    [SAVE]: { count: 1 },
    [`${SAVE} css(input[name="name"])`]: { count: 1, value: "Delivery", text: "" },
    [`${SAVE} testid(module-dialog-project)`]: { count: 1, text: "Website relaunch" },
    [`${SAVE} testid(module-dialog-status)`]: { count: 1, text: "Backlog" },
    [`${SAVE} testid(module-dialog-lead)`]: { count: 1, text: "Lead" },
    [`${SAVE} testid(module-dialog-members)`]: { count: 1, text: "Members" },
    [`${SAVE} testid(module-dialog-date-range)`]: { count: 1, text: "Start date End date" },
    [`${SAVE} css(textarea[name="description"])`]: { count: 1, value: "Notes" },
    [`${SAVE} role(alert,)`]: { count: 0 },
    "page role(option,)": { count: 1, evaluated: ["Website relaunch", "Project B"] },
    'page css([role="option"][aria-selected="true"])': { count: 0, evaluated: [] },
    "page role(listbox,Projects)": { count: 0 },
    ...extra,
  };
}

afterEach(() => {
  // Unmount first: the dialogs portal into body, and pulling a portal
  // container out from under React fails the unmount it is about to do.
  cleanup();
  vi.restoreAllMocks();
  resetAppClient();
  for (const stray of document.querySelectorAll("body > div:not([data-testid])")) stray.remove();
});

describe("header and zero-state creation entry points", () => {
  it("reads the header action label and clicks it", async () => {
    const fake = new FakePage({
      "page testid(modules-add)": { count: 1, text: " Add Module " },
      [SAVE]: { count: 1 },
    });
    const driver = new WebNewDriver(fake.asPage());
    expect(await driver.modulesAddLabel()).toBe("Add Module");
    await driver.modulesClickAdd();
    expect(fake.clicks).toContain("page testid(modules-add)");
  });

  it("reads a missing header action as null", async () => {
    const fake = new FakePage({ "page testid(modules-add)": { count: 0 } });
    expect(await new WebNewDriver(fake.asPage()).modulesAddLabel()).toBeNull();
  });

  it("opens the create dialog from the header and reads its heading", async () => {
    const fake = new FakePage({
      "page testid(modules-add)": { count: 1, text: "Add Module" },
      [SAVE]: { count: 1 },
      "page role(heading,Create module)": { count: 1, text: "Create module" },
    });
    const driver = new WebNewDriver(fake.asPage());
    await driver.modulesOpenCreateFromHeader();
    expect(fake.clicks).toContain("page testid(modules-add)");
    expect(await driver.modulesCreateDialogOpen()).toBe(true);
  });

  it("opens creation from the zero state and reads its copy", async () => {
    const zero = "page testid(modules-zero-state)";
    const fake = new FakePage({
      [zero]: { count: 1, visible: true },
      [`${zero} role(button,Set your first module)`]: { count: 1, disabled: false },
      [`${zero} css(img)`]: { count: 1, visible: true },
      [`${zero} css(h3 > span) nth(0)`]: { text: "Group work into modules" },
      [`${zero} css(h3 > span) nth(1)`]: { text: "Explain modules." },
      [SAVE]: { count: 1 },
    });
    const driver = new WebNewDriver(fake.asPage());
    expect(await driver.modulesListZeroStateVisible()).toBe(true);
    expect(await driver.modulesListZeroStateArtVisible()).toBe(true);
    expect(await driver.modulesListZeroStateCopy()).toEqual({
      title: "Group work into modules",
      description: "Explain modules.",
    });
    expect(await driver.modulesListZeroStateCreateDisabled()).toBe(false);
    await driver.modulesListZeroStateCreateOpen();
    expect(fake.clicks).toContain(`${zero} role(button,Set your first module)`);
    expect(await driver.modulesListCreateDialogVisible()).toBe(true);
  });

  it("reports the zero-state action disabled for a guest", async () => {
    const zero = "page testid(modules-zero-state)";
    const fake = new FakePage({ [`${zero} role(button,Set your first module)`]: { count: 1, disabled: true } });
    expect(await new WebNewDriver(fake.asPage()).modulesListZeroStateCreateDisabled()).toBe(true);
  });

  it("reports no art while the zero state is absent", async () => {
    const fake = new FakePage({ "page testid(modules-zero-state)": { count: 0 } });
    expect(await new WebNewDriver(fake.asPage()).modulesListZeroStateArtVisible()).toBe(false);
  });
});

describe("save dialog reads", () => {
  it("reads the project picker, options and makes a pick", async () => {
    const picker = `${SAVE} testid(module-dialog-project)`;
    const fake = new FakePage(saveDialogTable({ [picker]: { count: 1, text: "Project B" } }));
    const driver = new WebNewDriver(fake.asPage());
    expect(await driver.modulesDialogOpen()).toBe(true);
    expect(await driver.modulesDialogHasProjectPicker()).toBe(true);
    expect(await driver.modulesDialogProjectOptions()).toEqual(["Website relaunch", "Project B"]);
    await driver.modulesDialogPickProject("Project B");
    expect(fake.clicks).toContain(`${picker} role(button,)`);
  });

  it("reads title, description, status, lead, members and the date range", async () => {
    const fake = new FakePage(saveDialogTable());
    const driver = new WebNewDriver(fake.asPage());
    expect(await driver.modulesDialogTitle()).toBe("Delivery");
    expect(await driver.modulesDialogDescription()).toBe("Notes");
    expect(await driver.modulesDialogStatus()).toBe("Backlog");
    // Unset lead and members answer with their placeholders, as the oracle does.
    expect(await driver.modulesDialogLead()).toBe("Lead");
    expect(await driver.modulesDialogMembers()).toBe("Members");
    expect(await driver.modulesDialogDateRange()).toBe("Start date End date");
  });

  it("reads a set lead and member set from the selected option labels", async () => {
    const fake = new FakePage(
      saveDialogTable({
        'page css([role="option"][aria-selected="true"])': {
          count: 2,
          evaluated: ["You", "Pat Member"],
        },
      })
    );
    const driver = new WebNewDriver(fake.asPage());
    expect(await driver.modulesDialogLead()).toBe("You");
    expect(await driver.modulesDialogMembers()).toBe("You, Pat Member");
  });

  it("fills the title and description and reads the inline error", async () => {
    const fake = new FakePage(
      saveDialogTable({
        [`${SAVE} role(alert,)`]: { count: 1, text: " Title is required " },
      })
    );
    const driver = new WebNewDriver(fake.asPage());
    await driver.modulesDialogFillTitle("Refused");
    await driver.modulesDialogFillDescription("Details");
    expect(fake.fills).toEqual([
      `${SAVE} css(input[name="name"])=Refused`,
      `${SAVE} css(textarea[name="description"])=Details`,
    ]);
    expect(await driver.modulesDialogTitleError()).toBe("Title is required");
  });

  it("reads a title error as null when no alert renders", async () => {
    const fake = new FakePage(saveDialogTable());
    expect(await new WebNewDriver(fake.asPage()).modulesDialogTitleError()).toBeNull();
  });

  it("picks the status, lead and members", async () => {
    const fake = new FakePage(saveDialogTable());
    const driver = new WebNewDriver(fake.asPage());
    await driver.modulesDialogPickStatus("Backlog");
    await driver.modulesDialogPickLead("Lead");
    await driver.modulesDialogPickMembers(["Members"]);
    expect(fake.clicks).toContain(`${SAVE} testid(module-dialog-status) role(button,)`);
    expect(fake.clicks).toContain(`${SAVE} testid(module-dialog-lead) role(button,)`);
    expect(fake.clicks).toContain(`${SAVE} testid(module-dialog-members) role(button,)`);
    expect(fake.clicks).toContain("page role(option,Backlog)");
    expect(fake.presses).toContain("page:Escape");
  });

  it("opens the range calendar and picks both ends", async () => {
    const fake = new FakePage(
      saveDialogTable({
        [`${SAVE} testid(module-dialog-date-range) testid(module-dialog-date-range-trigger)`]: { count: 1 },
      })
    );
    await new WebNewDriver(fake.asPage()).modulesDialogPickDateRange("2026-10-01", "2026-10-31");
    expect(fake.clicks).toContain(`${SAVE} testid(module-dialog-date-range) testid(module-dialog-date-range-trigger)`);
    expect(fake.clicks).toContain('page testid(module-dialog-date-range-calendar) css(button[data-day="2026-10-31"])');
    expect(fake.presses).toContain("page:Escape");
  });

  it("reads the eight-stop tab order and the unnumbered project picker", async () => {
    const fake = new FakePage(
      saveDialogTable({
        [SAVE]: { count: 1, evaluated: [1, 2, 3, 4, 5, 6, 7, 8] },
        [`${SAVE} testid(module-dialog-project) role(button,)`]: { count: 1, attrs: { tabindex: "0" } },
      })
    );
    const driver = new WebNewDriver(fake.asPage());
    expect(await driver.modulesDialogTabOrder()).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
    expect(await driver.modulesDialogProjectPickerTabIndex()).toBe(0);
  });

  it("reads no picker tab index when the picker is absent", async () => {
    const fake = new FakePage({
      [SAVE]: { count: 1 },
      [`${SAVE} testid(module-dialog-project) role(button,)`]: { count: 0, attrs: { tabindex: null } },
    });
    expect(await new WebNewDriver(fake.asPage()).modulesDialogProjectPickerTabIndex()).toBeNull();
  });
});

describe("save dialog actions", () => {
  it("cancels and closes on Escape", async () => {
    const fake = new FakePage(saveDialogTable());
    const driver = new WebNewDriver(fake.asPage());
    await driver.modulesDialogCancel();
    await driver.modulesDialogEscape();
    expect(fake.clicks).toContain(`${SAVE} role(button,Cancel)`);
    expect(fake.presses).toContain("page:Escape");
  });

  it("submits creation and the update with their own labels", async () => {
    const fake = new FakePage(
      saveDialogTable({
        'page css([data-testid="toast-notification"])': { count: 1, text: "Success!\nModule saved." },
      })
    );
    const driver = new WebNewDriver(fake.asPage());
    await driver.modulesDialogSubmitCreate();
    await driver.modulesDialogSubmitUpdate();
    expect(fake.clicks).toContain(`${SAVE} role(button,Create Module)`);
    expect(fake.clicks).toContain(`${SAVE} role(button,Update Module)`);
  });

  it("reads the dialog kind from its heading", async () => {
    const fake = new FakePage({
      "page role(heading,Update module)": { count: 1, visible: true },
      "page role(heading,Delete module)": { count: 0, visible: false },
      'page css([role="dialog"]) role(heading,)': { count: 1, text: "Update module" },
    });
    const driver = new WebNewDriver(fake.asPage());
    expect(await driver.modulesDialogVisible("edit")).toBe(true);
    expect(await driver.modulesDialogVisible("delete")).toBe(false);
    expect(await driver.modulesDialogHeading()).toBe("Update module");
  });
});

describe("failure shaping", () => {
  function writeRoute(fake: FakePage): { handler: RouteHandler; matcher: unknown } {
    const armed = fake.routes[0];
    if (armed === undefined) throw new Error("no route armed");
    return { handler: armed.handler, matcher: armed.matcher };
  }

  it("aborts exactly the next create once and passes reads through", async () => {
    const fake = new FakePage();
    await new WebNewDriver(fake.asPage()).modulesFailNextCreate();
    const { handler, matcher } = writeRoute(fake);
    expect(typeof matcher).toBe("function");
    const matches = matcher as (url: URL) => boolean;
    expect(matches(new URL("https://app.test/api/workspaces/acme/projects/p1/modules/"))).toBe(true);
    expect(matches(new URL("https://app.test/api/workspaces/acme/projects/p1/modules/m1/"))).toBe(false);

    const events: string[] = [];
    const withEvents = (method: string): FakeRoute => ({
      ...route(method, "", handler),
      abort: async () => {
        events.push("abort");
      },
      continue: async () => {
        events.push("continue");
      },
    });
    await handler(withEvents("POST"));
    await handler(withEvents("POST"));
    await handler(withEvents("GET"));
    expect(events).toEqual(["abort", "continue", "continue"]);
  });

  it("aborts exactly the next update write once", async () => {
    const fake = new FakePage();
    await new WebNewDriver(fake.asPage()).modulesFailNextUpdate();
    const { handler, matcher } = writeRoute(fake);
    const matches = matcher as (url: URL) => boolean;
    expect(
      matches(new URL("https://app.test/api/workspaces/acme/projects/p1/modules/323e4567-e89b-12d3-a456-426614174002/"))
    ).toBe(true);
    const events: string[] = [];
    const withEvents = (method: string): FakeRoute => {
      const base = route(method, "", handler);
      return {
        ...base,
        abort: async () => {
          events.push("abort");
        },
        continue: async () => {
          events.push("continue");
        },
      };
    };
    await handler(withEvents("PATCH"));
    await handler(withEvents("PATCH"));
    await handler(withEvents("GET"));
    expect(events).toEqual(["abort", "continue", "continue"]);
  });

  it("arms a 403 delete refusal and a generic delete failure", async () => {
    const fake = new FakePage();
    const driver = new WebNewDriver(fake.asPage());
    await driver.modulesFailDeletePermission();
    await driver.modulesFailNextDelete();
    expect(fake.routes).toHaveLength(2);
    const fulfill = vi.fn(async () => undefined);
    for (const { handler, matcher } of fake.routes) {
      const matches = matcher as (url: URL) => boolean;
      const target = new URL(
        "https://app.test/api/workspaces/acme/projects/p1/modules/323e4567-e89b-12d3-a456-426614174002/"
      );
      expect(matches(target)).toBe(true);
      const deleteRoute = route("DELETE", "", handler);
      await handler({ ...deleteRoute, fulfill });
    }
    expect(fulfill).toHaveBeenNthCalledWith(1, expect.objectContaining({ status: 403 }));
    expect(fulfill).toHaveBeenNthCalledWith(2, expect.objectContaining({ status: 500 }));
  });
});

function renderDetailActions(props: { canEdit: boolean } = { canEdit: true }): void {
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith("/api/users/me/")) return jsonResponse(url, 500, {});
    return jsonResponse(url, 200, makeProjectRow());
  }) as never);
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ModuleDetailActions
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        moduleId={MODULES_MODULE_ID}
        canEdit={props.canEdit}
      />
    </QueryClientProvider>
  );
}

describe("detail menu and delete confirmation", () => {
  const triggerLocatorKey = `page css(${moduleActionsTriggerSelector({ visibleOnly: true })})`;

  it("asks the page for the composed trigger selector and clicks it", async () => {
    const fake = new FakePage({ [triggerLocatorKey]: { count: 1 }, "page role(menuitem,)": { count: 1 } });
    await new WebNewDriver(fake.asPage()).modulesDetailOpenMenu();
    expect(fake.clicks).toEqual([triggerLocatorKey]);
  });

  it("delegates the long-form detail menu opener", async () => {
    const fake = new FakePage({ [triggerLocatorKey]: { count: 1 }, "page role(menuitem,)": { count: 1 } });
    await new WebNewDriver(fake.asPage()).modulesOpenDetailMenu();
    expect(fake.clicks).toHaveLength(1);
  });

  // The fake page above answers whatever selector it is handed, so on its
  // own it can only show that the driver clicks what it asked for. These
  // cases run the selector over the real header markup instead: the
  // trigger is the menu's IconButton itself, so a descendant search inside
  // each scope child — the shape the old locator used — finds nothing.
  it("matches exactly the module actions trigger on the rendered detail menu", async () => {
    renderDetailActions();
    await screen.findByTestId("module-actions-trigger");

    const matches = document.querySelectorAll(moduleActionsTriggerSelector());
    expect(matches).toHaveLength(1);
    expect(matches[0]).toBe(screen.getByTestId("module-actions-trigger"));
    expect(matches[0]).toHaveAccessibleName("Module actions");

    const descendantMatches = Array.from(document.querySelectorAll('div[data-main-menu="true"] > button')).filter(
      (button) => button.querySelector('[data-testid="module-actions-trigger"]') !== null
    );
    expect(descendantMatches).toHaveLength(0);
  });

  it("ignores a module actions trigger outside the header menu", async () => {
    renderDetailActions();
    await screen.findByTestId("module-actions-trigger");
    const stray = document.createElement("div");
    stray.innerHTML = '<button data-testid="module-actions-trigger">Elsewhere</button>';
    document.body.append(stray);

    expect(document.querySelectorAll(moduleActionsTriggerSelector())).toHaveLength(1);
    expect(moduleActionsTriggerSelector({ visibleOnly: true })).toContain('div[data-main-menu="true"]:visible');
  });

  it("renders nothing for a guest, so the selector finds no trigger", () => {
    renderDetailActions({ canEdit: false });
    expect(document.querySelectorAll(moduleActionsTriggerSelector())).toHaveLength(0);
  });

  it("chooses a detail menu entry by title", async () => {
    const fake = new FakePage({ "page role(menuitem,Delete)": { count: 1 } });
    await new WebNewDriver(fake.asPage()).modulesChooseDetailMenuEntry("Delete");
    expect(fake.clicks).toContain("page role(menuitem,Delete)");
  });

  it("reads the delete dialog heading and body", async () => {
    const dialog = "page role(dialog,) has(page testid(module-delete-body),)";
    const fake = new FakePage({
      [dialog]: { count: 1 },
      [`${dialog} role(heading,)`]: { count: 1, text: " Delete module " },
      [`${dialog} testid(module-delete-body)`]: {
        count: 1,
        text: " Delivery and all of its data will be permanently removed. ",
      },
    });
    expect(await new WebNewDriver(fake.asPage()).modulesDeleteDialog()).toEqual({
      heading: "Delete module",
      body: "Delivery and all of its data will be permanently removed.",
    });
  });

  it("reports no delete dialog while none is open", async () => {
    const fake = new FakePage({ "page role(dialog,) has(page testid(module-delete-body),)": { count: 0 } });
    expect(await new WebNewDriver(fake.asPage()).modulesDeleteDialog()).toBeNull();
  });

  it("confirms and cancels the confirmation", async () => {
    const dialog = "page role(dialog,) has(page testid(module-delete-body),)";
    const fake = new FakePage({
      [dialog]: { count: 1 },
      [`${dialog} role(button,Delete)`]: { count: 1 },
      [`${dialog} role(button,Cancel)`]: { count: 1 },
    });
    const driver = new WebNewDriver(fake.asPage());
    await driver.modulesDeleteConfirm();
    await driver.modulesDeleteCancel();
    expect(fake.clicks).toContain(`${dialog} role(button,Delete)`);
    expect(fake.clicks).toContain(`${dialog} role(button,Cancel)`);
  });
});

describe("row facts and list state", () => {
  it("reads the row's name href and peek presence", async () => {
    const row = "page testid(modules-row) has(,Delivery)";
    const fake = new FakePage({
      [row]: { count: 1 },
      [`${row} testid(modules-row-link)`]: { attrs: { href: "/acme/projects/p1/modules/m1" } },
      [`${row} testid(modules-row-peek)`]: { count: 1 },
    });
    expect(await new WebNewDriver(fake.asPage()).modulesRowFacts("Delivery")).toEqual({
      nameHref: "/acme/projects/p1/modules/m1",
      hasPeek: true,
    });
  });

  it("throws when the row never rendered", async () => {
    const fake = new FakePage({ "page testid(modules-row) has(,Delivery)": { count: 0 } });
    await expect(new WebNewDriver(fake.asPage()).modulesRowFacts("Delivery")).rejects.toThrow(/never rendered/);
  });

  it("settles an empty project as the zero state", async () => {
    const fake = new FakePage({
      'page css([data-testid="modules-list"], [data-testid="modules-zero-state"], [data-testid="modules-no-match"], [data-testid="modules-feature-gate"])':
        {
          count: 1,
        },
      "page testid(modules-list)": { count: 0 },
      "page testid(modules-no-match)": { count: 0, visible: false },
      "page testid(modules-zero-state)": { count: 1, visible: true },
    });
    expect(await new WebNewDriver(fake.asPage()).modulesListEmptyKind()).toBe("zero");
  });
});

// The fake page above answers whatever selector the driver composes, so it
// can only show that a read asks for something. These cases render the
// real dialog and run the exact function the driver hands to
// `evaluateAll` over its markup: the picker rows and triggers carry a
// decorative project mark or avatar inside `aria-hidden`, and a scenario
// asserting a project or member name must not read that glyph.
describe("decoration-free text on the real dialog", () => {
  const PROJECT_B_ID = "623e4567-e89b-12d3-a456-426614174006";
  const ME = "423e4567-e89b-12d3-a456-426614174003";
  const TEAMMATE = "723e4567-e89b-12d3-a456-426614174007";
  const ME_MEMBERSHIP = "823e4567-e89b-12d3-a456-426614174008";
  const TEAMMATE_MEMBERSHIP = "923e4567-e89b-12d3-a456-426614174009";

  function projectRecord(id: string, name: string, identifier: string): Record<string, unknown> {
    return {
      id,
      name,
      identifier,
      sort_order: null,
      logo_props: {},
      member_role: 15,
      intake_count: 0,
      archived_at: null,
      workspace: MODULES_WS_ID,
      cycle_view: true,
      issue_views_view: true,
      module_view: true,
      page_view: false,
      inbox_view: false,
      is_default: false,
      guest_view_all_features: false,
      project_lead: null,
      network: 0,
      created_at: "2026-01-02T03:04:05Z",
      updated_at: "2026-02-03T04:05:06Z",
      created_by: null,
      updated_by: null,
    };
  }

  function directoryEntry(id: string, first: string, last: string, membershipId: string): Record<string, unknown> {
    return {
      id: membershipId,
      role: 15,
      member: {
        id,
        first_name: first,
        last_name: last,
        avatar: "",
        avatar_url: null,
        display_name: `${first} ${last}`,
        is_bot: false,
      },
    };
  }

  beforeEach(() => {
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
      if (url === "https://api.test/api/workspaces/acme/projects/") {
        return jsonResponse(url, 200, [
          projectRecord(MODULES_PROJECT_ID, "Website relaunch", "WEB"),
          projectRecord(PROJECT_B_ID, "Project B", "PB"),
        ]);
      }
      if (url.endsWith("/api/users/me/")) {
        return jsonResponse(url, 200, {
          id: ME,
          email: "owner@test.dev",
          username: "owner",
          first_name: "Rich",
          last_name: "Owner",
          display_name: "Rich Owner",
          avatar: "",
          avatar_url: null,
          cover_image: null,
          cover_image_url: null,
          date_joined: "2026-01-02T03:04:05Z",
          user_timezone: "UTC",
          is_active: true,
          is_bot: false,
          is_email_verified: true,
          is_password_autoset: false,
          last_login_medium: "password",
          last_login_time: null,
        });
      }
      if (url.endsWith("/members/") && url.includes("/projects/")) {
        return jsonResponse(url, 200, [
          {
            id: ME_MEMBERSHIP,
            role: 15,
            member: ME,
            project: MODULES_PROJECT_ID,
            original_role: 15,
            created_at: "2026-01-02T03:04:05Z",
          },
          {
            id: TEAMMATE_MEMBERSHIP,
            role: 15,
            member: TEAMMATE,
            project: MODULES_PROJECT_ID,
            original_role: 15,
            created_at: "2026-01-02T03:04:05Z",
          },
        ]);
      }
      if (url.endsWith("/api/workspaces/acme/members/")) {
        return jsonResponse(url, 200, [
          directoryEntry(ME, "Rich", "Owner", ME_MEMBERSHIP),
          directoryEntry(TEAMMATE, "Pat", "Member", TEAMMATE_MEMBERSHIP),
        ]);
      }
      throw new Error(`unexpected request: ${url}`);
    }) as never);
  });

  function renderSaveDialog(): void {
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <ModuleSaveDialog
          open
          onOpenChange={() => undefined}
          mode="create"
          workspaceSlug="acme"
          currentProject={{ id: MODULES_PROJECT_ID, name: "Website relaunch", identifier: "WEB" }}
        />
      </QueryClientProvider>
    );
  }

  /** What the driver's read answers for one control's markup. */
  function controlText(testId: string): string {
    return moduleVisibleTexts(Array.from(document.querySelectorAll(`[data-testid="${testId}"]`))).join(" ");
  }

  function openPicker(testId: string): void {
    fireEvent.click(within(screen.getByTestId(testId)).getByRole("button"));
  }

  it("names project options without the decorative mark", async () => {
    renderSaveDialog();
    openPicker("module-dialog-project");
    await screen.findAllByRole("option");

    const options = Array.from(document.querySelectorAll('[role="option"]'));
    expect(moduleVisibleTexts(options)).toEqual(["Website relaunch", "Project B"]);
    // The raw text is what the old read returned, and why the helper exists.
    expect(options.map((option) => option.textContent)).toEqual(["WWebsite relaunch", "PProject B"]);
  });

  it("names member options without the avatar initials", async () => {
    renderSaveDialog();
    openPicker("module-dialog-lead");
    await screen.findAllByRole("option");

    const options = Array.from(document.querySelectorAll('[role="option"]'));
    expect(moduleVisibleTexts(options)).toEqual(["You", "Pat Member"]);
    expect(options.map((option) => option.textContent)).toEqual(["ROYou", "PMPat Member"]);
  });

  it("reads a selected lead and members from the selected options", async () => {
    renderSaveDialog();
    openPicker("module-dialog-lead");
    fireEvent.click(await screen.findByRole("option", { name: "You", exact: true }));
    // A single pick closes the popup, so the read reopens it to ask the
    // option list what is set — the same order the driver uses.
    openPicker("module-dialog-lead");
    await screen.findAllByRole("option");
    expect(moduleVisibleTexts(Array.from(document.querySelectorAll(MODULE_SELECTED_OPTION_SELECTOR)))).toEqual(["You"]);
    fireEvent.keyDown(document.body, { key: "Escape" });

    openPicker("module-dialog-members");
    fireEvent.click(await screen.findByRole("option", { name: "Pat Member", exact: true }));
    // Multi-select stays open across picks, so its selection reads in place.
    expect(moduleVisibleTexts(Array.from(document.querySelectorAll(MODULE_SELECTED_OPTION_SELECTOR)))).toEqual([
      "Pat Member",
    ]);
    // The trigger names the member without the avatar initials; it is not
    // what the read answers, which keeps "You" an option label, as the
    // oracle reads it.
    expect(controlText("module-dialog-lead")).toBe("Rich Owner");
  });

  it("reads the status, the empty date range, and the unset pickers as the scenario spells them", () => {
    renderSaveDialog();
    expect(controlText("module-dialog-status")).toBe("Backlog");
    expect(controlText("module-dialog-date-range")).toBe("Start date End date");
    expect(controlText("module-dialog-lead")).toBe("Lead");
    expect(controlText("module-dialog-members")).toBe("Members");
  });

  it("keeps the zero state's copy and action label intact", () => {
    const onCreate = vi.fn();
    render(<ModulesZeroState canCreate onCreate={onCreate} />);
    const zero = screen.getByTestId("modules-zero-state");
    expect(moduleVisibleTexts(Array.from(zero.querySelectorAll("button")))).toEqual(["Set your first module"]);
  });
});
