// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The modules list screen composes exactly one body state (MOD-001/002/
// 008/011): the flag gate once the project says the feature is off — with
// no list read at all — the layout-matched skeleton while the project or
// the list loads, and the content slot once both settle. The layout choice
// is the viewer's per-project record, restored on mount and followed by the
// skeleton and the body. Slice B adds the header controls: the search,
// ordering and filters write the same per-project display record, the chip
// strip follows the applied filters, and the narrowed body (or the no-match
// container) reads the context those controls provide.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const routerHarness = vi.hoisted(() => ({ navigate: vi.fn() }));
const viewportHarness = vi.hoisted(() => ({ narrow: false }));

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  Link: ({ to, children, className, ...rest }: { to: string; children?: React.ReactNode; className?: string }) => (
    <a href={to} className={className} {...rest}>
      {children}
    </a>
  ),
  useNavigate: () => routerHarness.navigate,
}));

// The panel's own suite covers its body; the screen's contract is which
// module it mounts, with which fallback name, and where the close writes.
vi.mock("./ModulePeekPanel.js", () => ({
  ModulePeekPanel: (props: { moduleId: string; fallbackName?: string; archived?: boolean; onClose: () => void }) => (
    <div
      data-testid="peek-stub"
      data-module-id={props.moduleId}
      data-fallback-name={props.fallbackName ?? ""}
      data-archived={String(props.archived === true)}
    >
      <button type="button" onClick={props.onClose}>
        Close stub
      </button>
    </div>
  ),
}));

vi.mock("../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useNarrowViewport: () => viewportHarness.narrow,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { readProjectLayout, writeProjectLayout } from "../lib/listLayoutStore.js";
import { readProjectDisplay, resetModuleDisplayCache } from "../lib/moduleListDisplayStore.js";
import { jsonResponse, makeModuleRow, makeProjectRow, MODULES_MODULE_ID, MODULES_PROJECT_ID } from "../testing.js";
import { ModulesListScreen } from "./ModulesListScreen.js";

interface Harness {
  project: Record<string, unknown>;
  projectDelayMs: number;
  projectStatus: number;
  roles: Record<string, number>;
  modulesDelayMs: number;
  modulesStatus: number;
  meStatus: number;
  modulesCalls: number;
  modules: ReturnType<typeof makeModuleRow>[];
  createStatus: number;
  writes: { method: string; url: string; body: Record<string, unknown> | null }[];
}

const ME_ID = "423e4567-e89b-12d3-a456-426614174003";
const TEAMMATE_ID = "623e4567-e89b-12d3-a456-426614174005";

function directoryMember(id: string, display: string, membershipId: string) {
  return {
    id: membershipId,
    role: 15,
    member: {
      id,
      first_name: display,
      last_name: "",
      avatar: "",
      avatar_url: null,
      display_name: display,
      is_bot: false,
    },
  };
}

const ME = {
  id: ME_ID,
  email: "rich@test.dev",
  username: "rich",
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
};

const PROFILE = {
  id: "523e4567-e89b-12d3-a456-426614174004",
  user: ME_ID,
  theme: {},
  is_app_rail_docked: false,
  is_tour_completed: false,
  onboarding_step: {},
  use_case: null,
  role: null,
  is_onboarded: true,
  last_workspace_id: null,
  billing_address_country: "",
  billing_address: null,
  has_billing_address: false,
  company_name: "",
  notification_view_mode: "all",
  is_smooth_cursor_enabled: false,
  is_mobile_onboarded: false,
  mobile_onboarding_step: {},
  mobile_timezone_auto_set: true,
  language: "en",
  start_of_the_week: 0,
  goals: {},
  background_color: "",
  is_navigation_tour_completed: false,
  has_marketing_email_consent: false,
  is_subscribed_to_changelog: false,
  product_tour: {},
  settings: {},
  created_at: "2026-01-02T03:04:05Z",
  updated_at: "2026-02-03T04:05:06Z",
};

let harness: Harness;

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

beforeEach(() => {
  hostReset();
  resetModuleDisplayCache();
  globalThis.localStorage.clear();
  routerHarness.navigate.mockReset();
  viewportHarness.narrow = false;
  harness = {
    project: makeProjectRow(),
    projectDelayMs: 0,
    projectStatus: 200,
    roles: { [MODULES_PROJECT_ID]: 20 },
    modulesDelayMs: 0,
    modulesStatus: 200,
    meStatus: 200,
    modulesCalls: 0,
    modules: [makeModuleRow()],
    createStatus: 201,
    writes: [],
  };
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(`/api/workspaces/acme/projects/${MODULES_PROJECT_ID}/`)) {
      if (harness.projectDelayMs > 0) await sleep(harness.projectDelayMs);
      return jsonResponse(url, harness.projectStatus, harness.project);
    }
    if (url.endsWith("/project-roles/")) return jsonResponse(url, 200, harness.roles);
    if (url.endsWith("/api/workspaces/acme/members/")) {
      return jsonResponse(url, 200, [
        directoryMember(ME_ID, "Rich Owner", "123e4567-e89b-12d3-a456-426614174101"),
        directoryMember(TEAMMATE_ID, "Pat Member", "123e4567-e89b-12d3-a456-426614174102"),
      ]);
    }
    if (url.endsWith("/api/users/me/")) return jsonResponse(url, harness.meStatus, ME);
    if (url.endsWith("/api/users/me/profile/")) return jsonResponse(url, 200, PROFILE);
    if ((init.method ?? "GET") === "POST" && url.endsWith(`/projects/${MODULES_PROJECT_ID}/modules/`)) {
      const body = init.body === undefined ? null : (JSON.parse(init.body) as Record<string, unknown>);
      harness.writes.push({ method: "POST", url, body });
      if (harness.createStatus >= 400) return jsonResponse(url, harness.createStatus, { error: "Duplicate." });
      const created = makeModuleRow({
        id: "323e4567-e89b-12d3-a456-426614174099",
        name: String(body?.["name"] ?? "Created"),
      });
      harness.modules = [...harness.modules, created];
      return jsonResponse(url, harness.createStatus, created);
    }
    if (url.endsWith(`/api/workspaces/acme/projects/${MODULES_PROJECT_ID}/modules/`)) {
      harness.modulesCalls += 1;
      if (harness.modulesDelayMs > 0) await sleep(harness.modulesDelayMs);
      return jsonResponse(url, harness.modulesStatus, harness.modules);
    }
    if (url.endsWith(`/projects/${MODULES_PROJECT_ID}/archived-modules/`)) {
      return jsonResponse(url, 200, []);
    }
    if (url.endsWith("/api/workspaces/acme/projects/")) {
      return jsonResponse(url, 200, [makeProjectRow()]);
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

/** Separate hook so the module-level setup order stays obvious. */
function hostReset(): void {
  resetAppClient();
}

afterEach(() => {
  resetAppClient();
  resetModuleDisplayCache();
  document.title = "";
});

function renderScreen(
  children?: React.ReactNode,
  props: Partial<React.ComponentProps<typeof ModulesListScreen>> = {}
): ReturnType<typeof render> {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ModulesListScreen workspaceSlug="acme" projectId={MODULES_PROJECT_ID} {...props}>
        {children}
      </ModulesListScreen>
    </QueryClientProvider>
  );
}

describe("ModulesListScreen", () => {
  it("renders the rows, the breadcrumb and the tab title once settled", async () => {
    renderScreen();
    expect(await screen.findByTestId("modules-list")).toHaveTextContent("Delivery");
    expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent("Modules");
    expect(document.title).toBe("Website relaunch - Modules");
    expect(screen.queryByTestId("modules-list-skeleton")).not.toBeInTheDocument();
    expect(screen.queryByTestId("modules-feature-gate")).not.toBeInTheDocument();
  });

  it("renders its own body when no content slot is supplied", async () => {
    renderScreen();
    expect(await screen.findByTestId("modules-row-name")).toHaveTextContent("Delivery");
    expect(screen.getByTestId("modules-row-link")).toHaveAttribute(
      "href",
      `/acme/projects/${MODULES_PROJECT_ID}/modules/${MODULES_MODULE_ID}`
    );
  });

  it("mounts the shared quick menu on every row without a host menu", async () => {
    renderScreen();
    expect(await screen.findByTestId("modules-row")).toBeInTheDocument();
    // One row mounts two triggers: the narrow-screen and desktop mounts.
    const triggers = screen.getAllByTestId("module-row-menu-trigger");
    expect(triggers).toHaveLength(2);
    expect(triggers[0]).toHaveAccessibleName("Module actions for Delivery");
  });

  it("opens the editor entries from the default row menu", async () => {
    renderScreen();
    const triggers = await screen.findAllByTestId("module-row-menu-trigger");
    fireEvent.click(triggers[0]!);
    const titles = screen
      .getAllByRole("menuitem")
      .map((entry) => entry.querySelector("[data-menu-title]")?.textContent?.trim() ?? entry.textContent?.trim());
    expect(titles).toEqual(["Edit", "Open in new tab", "Copy link", "Archive", "Delete"]);
  });

  it("mounts the shared quick menu on board cards", async () => {
    writeProjectLayout(MODULES_PROJECT_ID, "board");
    renderScreen();
    const card = await screen.findByTestId("modules-board-card");
    expect(within(card).getByTestId("module-row-menu-trigger")).toBeInTheDocument();
  });

  it("hands the rows their host menu slot when a host supplies one", async () => {
    renderScreen(undefined, { rowMenu: (module) => <button type="button">{`Menu ${module.name}`}</button> });
    const slot = await screen.findByTestId("modules-row-menu");
    expect(within(slot).getByRole("button", { name: "Menu Delivery" })).toBeInTheDocument();
    expect(screen.queryByTestId("module-row-menu-trigger")).not.toBeInTheDocument();
  });

  it("gives the board cards their lead from the workspace directory", async () => {
    harness.modules = [
      makeModuleRow({ name: "Led", lead_id: TEAMMATE_ID }),
      makeModuleRow({ id: "323e4567-e89b-12d3-a456-426614174011", name: "Unled" }),
    ];
    writeProjectLayout(MODULES_PROJECT_ID, "board");
    renderScreen();
    const cards = await screen.findAllByTestId("modules-board-card");
    expect(within(cards[0]!).getByTestId("modules-card-lead")).toHaveAttribute("data-lead-kind", "avatar");
    expect(within(cards[1]!).getByTestId("modules-card-lead")).toHaveAttribute("data-lead-kind", "none");
  });

  it("gates the screen with an admin shortcut and never reads the list", async () => {
    harness.project = makeProjectRow({ module_view: false });
    renderScreen();
    expect(await screen.findByTestId("modules-feature-gate")).toBeInTheDocument();
    expect(screen.queryByTestId("modules-list")).not.toBeInTheDocument();
    expect(screen.queryByTestId("modules-list-header")).not.toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Module layout" })).not.toBeInTheDocument();
    expect(harness.modulesCalls).toBe(0);
    fireEvent.click(screen.getByRole("button", { name: "Manage features" }));
    expect(routerHarness.navigate).toHaveBeenCalledWith({
      to: `/acme/settings/projects/${MODULES_PROJECT_ID}/features/modules`,
    });
  });

  it("disables the gate shortcut for non-admins", async () => {
    harness.project = makeProjectRow({ module_view: false });
    harness.roles = { [MODULES_PROJECT_ID]: 15 };
    renderScreen();
    expect(await screen.findByTestId("modules-feature-gate")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Manage features" })).toBeDisabled();
  });

  it("shows the skeleton, never the gate, while the project loads", async () => {
    harness.projectDelayMs = 50;
    renderScreen();
    expect(await screen.findByTestId("modules-list-skeleton")).toBeInTheDocument();
    expect(screen.queryByTestId("modules-feature-gate")).not.toBeInTheDocument();
    expect(await screen.findByTestId("modules-list")).toBeInTheDocument();
    expect(screen.queryByTestId("modules-list-skeleton")).not.toBeInTheDocument();
  });

  it("shows the layout-matched skeleton while the list loads, exclusive of rows", async () => {
    harness.modulesDelayMs = 50;
    writeProjectLayout(MODULES_PROJECT_ID, "board");
    renderScreen();
    const skeleton = await screen.findByTestId("modules-list-skeleton");
    expect(skeleton).toHaveAttribute("data-layout", "board");
    expect(screen.queryAllByTestId("modules-row")).toHaveLength(0);
    expect(await screen.findByTestId("modules-board-card")).toBeInTheDocument();
    expect(screen.queryByTestId("modules-list-skeleton")).not.toBeInTheDocument();
  });

  it("renders the content slot instead of its own rows", async () => {
    renderScreen(<div data-testid="modules-slot">custom</div>);
    expect(await screen.findByTestId("modules-slot")).toBeInTheDocument();
    expect(screen.queryByTestId("modules-list")).not.toBeInTheDocument();
  });

  it("restores a remembered layout and switches it through the header", async () => {
    writeProjectLayout(MODULES_PROJECT_ID, "timeline");
    renderScreen();
    const list = await screen.findByTestId("modules-list");
    expect(list).toHaveAttribute("data-layout", "timeline");
    fireEvent.click(screen.getByRole("button", { name: "Board layout" }));
    await waitFor(() => expect(screen.getByTestId("modules-list")).toHaveAttribute("data-layout", "board"));
    expect(readProjectLayout(MODULES_PROJECT_ID)).toBe("board");
    expect(harness.modulesCalls).toBe(1);
  });

  it("renders list and gallery only on small screens", async () => {
    viewportHarness.narrow = true;
    writeProjectLayout(MODULES_PROJECT_ID, "timeline");
    renderScreen();
    const list = await screen.findByTestId("modules-list");
    expect(list).toHaveAttribute("data-layout", "list");
    fireEvent.click(screen.getByRole("button", { name: "Change module layout, current List" }));
    const menu = within(screen.getByRole("menu", { name: "Change module layout, current List" }));
    expect(menu.getAllByRole("menuitem")).toHaveLength(2);
    expect(readProjectLayout(MODULES_PROJECT_ID)).toBe("timeline");
  });

  it("gates a project row that carries no module_view flag", async () => {
    const { module_view: _omitted, ...row } = makeProjectRow();
    harness.project = row;
    renderScreen();
    expect(await screen.findByTestId("modules-feature-gate")).toBeInTheDocument();
    expect(screen.queryByTestId("modules-list-skeleton")).not.toBeInTheDocument();
    expect(screen.queryByTestId("modules-list")).not.toBeInTheDocument();
    expect(harness.modulesCalls).toBe(0);
  });

  it("gates a parseable row whose module_view is null or not boolean", async () => {
    harness.project = makeProjectRow({ module_view: null });
    const first = renderScreen();
    expect(await screen.findByTestId("modules-feature-gate")).toBeInTheDocument();
    expect(harness.modulesCalls).toBe(0);
    first.unmount();
    harness.project = makeProjectRow({ module_view: "on" });
    renderScreen();
    expect(await screen.findByTestId("modules-feature-gate")).toBeInTheDocument();
    expect(harness.modulesCalls).toBe(0);
  });

  it("disables the gate shortcut when the role map names no role", async () => {
    harness.project = makeProjectRow({ module_view: false });
    harness.roles = {};
    renderScreen();
    expect(await screen.findByTestId("modules-feature-gate")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Manage features" })).toBeDisabled();
  });

  it("shows an empty list rather than a skeleton when the list read fails", async () => {
    harness.modulesStatus = 500;
    renderScreen();
    const list = await screen.findByTestId("modules-list");
    expect(list).toHaveAttribute("data-layout", "list");
    expect(within(list).queryAllByTestId("modules-row")).toHaveLength(0);
    expect(screen.queryByTestId("modules-list-skeleton")).not.toBeInTheDocument();
  });

  it("mounts the header Add action for an admin and opens the create dialog", async () => {
    renderScreen();
    await screen.findByTestId("modules-list");
    fireEvent.click(screen.getByRole("button", { name: "Add Module" }));
    expect(await screen.findByRole("heading", { name: "Create module" })).toBeInTheDocument();
    expect(within(screen.getByTestId("module-dialog-project")).getByRole("button")).toHaveTextContent(
      "Website relaunch"
    );
  });

  it("keeps the creation action for a member", async () => {
    harness.roles = { [MODULES_PROJECT_ID]: 15 };
    renderScreen();
    await screen.findByTestId("modules-list");
    expect(screen.getByRole("button", { name: "Add Module" })).toBeEnabled();
  });

  it("never offers creation to a guest", async () => {
    harness.roles = { [MODULES_PROJECT_ID]: 5 };
    renderScreen();
    await screen.findByTestId("modules-list");
    expect(screen.queryByTestId("modules-add")).not.toBeInTheDocument();
  });

  it("shortens the Add label on small screens", async () => {
    viewportHarness.narrow = true;
    renderScreen();
    await screen.findByTestId("modules-list");
    expect(screen.getByRole("button", { name: "Add" })).toHaveAttribute("data-testid", "modules-add");
    expect(screen.queryByRole("button", { name: "Add Module" })).not.toBeInTheDocument();
  });

  it("shows the onboarding state for a project with no modules and creates from it", async () => {
    harness.modules = [];
    renderScreen();
    const zero = await screen.findByTestId("modules-zero-state");
    expect(screen.queryByTestId("modules-list")).not.toBeInTheDocument();
    const action = within(zero).getByRole("button", { name: "Set your first module" });
    expect(action).toBeEnabled();
    fireEvent.click(action);
    expect(await screen.findByRole("heading", { name: "Create module" })).toBeInTheDocument();
  });

  it("disables the onboarding action for a guest", async () => {
    harness.modules = [];
    harness.roles = { [MODULES_PROJECT_ID]: 5 };
    renderScreen();
    const zero = await screen.findByTestId("modules-zero-state");
    expect(within(zero).getByRole("button", { name: "Set your first module" })).toBeDisabled();
  });

  it("keeps the zero state instead of the no-match container when no filter is active", async () => {
    harness.modules = [];
    renderScreen();
    await screen.findByTestId("modules-zero-state");
    expect(screen.queryByTestId("modules-no-match")).not.toBeInTheDocument();
  });

  it("refreshes the list after a create and shows the new module", async () => {
    harness.modules = [];
    renderScreen();
    fireEvent.click(await screen.findByRole("button", { name: "Set your first module" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Title" }), {
      target: { value: "Fresh Module" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Create Module" }));
    expect(await screen.findByTestId("modules-row-name")).toHaveTextContent("Fresh Module");
    expect(harness.writes).toHaveLength(1);
    expect(harness.writes[0]?.body).toMatchObject({ name: "Fresh Module", status: "backlog" });
    expect(screen.queryByRole("heading", { name: "Create module" })).not.toBeInTheDocument();
  });

  it("keeps the create dialog up with its input when the write fails", async () => {
    harness.modules = [];
    harness.createStatus = 400;
    renderScreen();
    fireEvent.click(await screen.findByRole("button", { name: "Set your first module" }));
    const dialog = await screen.findByRole("dialog");
    const title = within(dialog).getByRole("textbox", { name: "Title" });
    fireEvent.change(title, { target: { value: "Duplicate" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Create Module" }));
    await waitFor(() => expect(harness.writes).toHaveLength(1));
    expect(await screen.findByRole("heading", { name: "Create module" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Title" })).toHaveValue("Duplicate");
    expect(screen.getByTestId("modules-zero-state")).toBeInTheDocument();
  });

  it("falls back to the crumb as the title before the project arrives", async () => {
    harness.projectDelayMs = 50;
    renderScreen();
    await waitFor(() => expect(document.title).toBe("Modules"));
    await waitFor(() => expect(document.title).toBe("Website relaunch - Modules"));
  });

  it("narrows the rows through the header search and restores them when cleared", async () => {
    harness.modules = [
      makeModuleRow({ name: "Alpha" }),
      makeModuleRow({ id: "323e4567-e89b-12d3-a456-426614174009", name: "Bravo" }),
    ];
    renderScreen();
    await screen.findByTestId("modules-list");
    fireEvent.click(screen.getByRole("button", { name: "Search modules" }));
    const field = screen.getByRole("textbox", { name: "Search modules by name" });
    fireEvent.change(field, { target: { value: "alph" } });
    await waitFor(() =>
      expect(screen.getAllByTestId("modules-row-name").map((node) => node.textContent)).toEqual(["Alpha"])
    );
    expect(readProjectDisplay(MODULES_PROJECT_ID).query).toBe("alph");
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Search modules by name" }), { key: "Escape" });
    await waitFor(() =>
      expect(screen.getAllByTestId("modules-row-name").map((node) => node.textContent)).toEqual(["Alpha", "Bravo"])
    );
  });

  it("shows the no-match container when the search hides every row", async () => {
    renderScreen();
    await screen.findByTestId("modules-list");
    fireEvent.click(screen.getByRole("button", { name: "Search modules" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Search modules by name" }), { target: { value: "zzz" } });
    expect(await screen.findByTestId("modules-no-match")).toBeInTheDocument();
    expect(screen.queryByTestId("modules-list")).not.toBeInTheDocument();
    expect(screen.getByTestId("modules-list-header")).toBeInTheDocument();
  });

  it("writes the chosen ordering to the display record", async () => {
    renderScreen();
    await screen.findByTestId("modules-list");
    expect(screen.getByTestId("modules-sort-label")).toHaveTextContent("Name");
    fireEvent.click(screen.getByTestId("modules-sort-trigger"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Created date" }));
    await waitFor(() => expect(readProjectDisplay(MODULES_PROJECT_ID).sort.key).toBe("created-date"));
    expect(screen.getByTestId("modules-sort-label")).toHaveTextContent("Created date");
  });

  it("raises the chip strip with the applied filters and clears it through clear-all", async () => {
    harness.modules = [
      makeModuleRow({ name: "Done", status: "completed" }),
      makeModuleRow({ id: "323e4567-e89b-12d3-a456-426614174010", name: "Open", status: "planned" }),
    ];
    renderScreen();
    await screen.findByTestId("modules-list");
    expect(screen.queryByTestId("modules-filter-chips")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    fireEvent.click(screen.getByRole("checkbox", { name: "Completed" }));
    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    await waitFor(() =>
      expect(screen.getAllByTestId("modules-row-name").map((node) => node.textContent)).toEqual(["Done"])
    );
    expect(screen.getByTestId("modules-filter-active")).toBeInTheDocument();
    expect(screen.getAllByTestId("modules-filter-chip").map((node) => node.getAttribute("data-chip-key"))).toEqual([
      "status",
    ]);
    fireEvent.click(screen.getByTestId("modules-filter-clear-all"));
    await waitFor(() =>
      expect(screen.getAllByTestId("modules-row-name").map((node) => node.textContent)).toEqual(["Done", "Open"])
    );
    expect(screen.queryByTestId("modules-filter-chips")).not.toBeInTheDocument();
    expect(screen.queryByTestId("modules-filter-active")).not.toBeInTheDocument();
  });

  it("keeps the filters panel open while several statuses are picked", async () => {
    // The panel must survive the re-render a selection causes: an inline
    // component type here would remount it and close it after one pick.
    harness.modules = [
      makeModuleRow({ name: "Done", status: "completed" }),
      makeModuleRow({ id: "323e4567-e89b-12d3-a456-426614174010", name: "Started", status: "in-progress" }),
      makeModuleRow({ id: "323e4567-e89b-12d3-a456-426614174011", name: "Later", status: "backlog" }),
    ];
    renderScreen();
    await screen.findByTestId("modules-list");
    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    fireEvent.click(screen.getByRole("checkbox", { name: "Completed" }));
    expect(screen.getByTestId("modules-filter-panel")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: "In Progress" }));
    expect(screen.getByTestId("modules-filter-panel")).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Completed" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("checkbox", { name: "In Progress" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("checkbox", { name: "Backlog" })).toHaveAttribute("aria-checked", "false");
    await waitFor(() =>
      expect(screen.getAllByTestId("modules-row-name").map((node) => node.textContent)).toEqual(["Done", "Started"])
    );
    expect(readProjectDisplay(MODULES_PROJECT_ID).filters.status).toEqual(["completed", "in-progress"]);
  });

  it("offers the current user first in the people filters once the directory loads", async () => {
    renderScreen();
    await screen.findByTestId("modules-list");
    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    // The popover's own chrome is aria-hidden while it animates in, so read
    // the group's rows by their stable attributes.
    await waitFor(() => {
      const lead = screen
        .getAllByTestId("modules-filter-group")
        .find((node) => node.getAttribute("data-filter-group") === "Lead")!;
      expect([...lead.querySelectorAll('[role="checkbox"]')].map((node) => node.getAttribute("aria-label"))).toEqual([
        "You",
        "Pat Member",
      ]);
    });
  });

  it("keeps the controls usable before the viewer resolves, without a You row", async () => {
    // A failed viewer read leaves meId null; the directory still offers names.
    harness.meStatus = 500;
    harness.modules = [makeModuleRow({ name: "Undecided Owner" })];
    renderScreen();
    await screen.findByTestId("modules-list");
    fireEvent.click(screen.getByTestId("modules-filter-trigger"));
    await waitFor(() => {
      const lead = screen
        .getAllByTestId("modules-filter-group")
        .find((node) => node.getAttribute("data-filter-group") === "Lead")!;
      expect(
        [...lead.querySelectorAll('[role="checkbox"]')].map((node) => node.getAttribute("aria-label"))
      ).not.toContain("You");
    });
  });

  it("opens the peek through the address writer and mounts the panel", async () => {
    const onPeekChange = vi.fn();
    renderScreen(undefined, { peekModuleId: null, onPeekChange });
    await screen.findByTestId("modules-list");
    expect(screen.queryByTestId("modules-peek")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("modules-row-peek"));
    expect(onPeekChange).toHaveBeenCalledWith(MODULES_MODULE_ID);
  });

  it("closes the peek already open from the same control", async () => {
    const onPeekChange = vi.fn();
    renderScreen(undefined, { peekModuleId: MODULES_MODULE_ID, onPeekChange });
    await screen.findByTestId("modules-list");
    fireEvent.click(screen.getByTestId("modules-row-peek"));
    expect(onPeekChange).toHaveBeenCalledWith(null);
  });

  it("mounts the peek panel for the module the address names", async () => {
    renderScreen(undefined, { peekModuleId: MODULES_MODULE_ID, onPeekChange: vi.fn() });
    await screen.findByTestId("modules-list");
    const panel = await screen.findByTestId("peek-stub");
    expect(panel).toHaveAttribute("data-module-id", MODULES_MODULE_ID);
    expect(panel).toHaveAttribute("data-fallback-name", "Delivery");
    expect(panel).toHaveAttribute("data-archived", "false");
  });

  it("closes the panel through the address writer", async () => {
    const onPeekChange = vi.fn();
    renderScreen(undefined, { peekModuleId: MODULES_MODULE_ID, onPeekChange });
    await screen.findByTestId("modules-list");
    fireEvent.click(screen.getByRole("button", { name: "Close stub" }));
    expect(onPeekChange).toHaveBeenCalledWith(null);
  });

  it("never mounts the panel in archived mode, which opens the archived peek itself", async () => {
    renderScreen(undefined, { peekModuleId: MODULES_MODULE_ID, onPeekChange: vi.fn(), archived: true });
    await screen.findByTestId("modules-list");
    expect(screen.queryByTestId("modules-peek")).not.toBeInTheDocument();
    expect(screen.getByTestId("modules-row")).toHaveAttribute("data-module-archived", "true");
  });

  it("gives the rows a working info control even when no address writer is supplied", async () => {
    renderScreen();
    await screen.findByTestId("modules-list");
    // The control swallows its press but has nowhere to write, so the
    // panel stays shut and the row is still on the list.
    expect(fireEvent.click(screen.getByTestId("modules-row-peek"))).toBe(false);
    expect(screen.queryByTestId("peek-stub")).not.toBeInTheDocument();
    expect(screen.getByTestId("modules-row")).toBeInTheDocument();
  });
});
