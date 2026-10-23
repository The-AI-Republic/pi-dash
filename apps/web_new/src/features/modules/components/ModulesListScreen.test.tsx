// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The modules list screen composes exactly one body state (MOD-001/002/
// 008/011): the flag gate once the project says the feature is off — with
// no list read at all — the layout-matched skeleton while the project or
// the list loads, and the content slot once both settle. The layout choice
// is the viewer's per-project record, restored on mount and followed by the
// skeleton and the body.

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

vi.mock("../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useNarrowViewport: () => viewportHarness.narrow,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { readProjectLayout, writeProjectLayout } from "../lib/listLayoutStore.js";
import { jsonResponse, makeModuleRow, makeProjectRow, MODULES_MODULE_ID, MODULES_PROJECT_ID } from "../testing.js";
import { ModulesListScreen } from "./ModulesListScreen.js";

interface Harness {
  project: Record<string, unknown>;
  projectDelayMs: number;
  projectStatus: number;
  roles: Record<string, number>;
  modulesDelayMs: number;
  modulesStatus: number;
  modulesCalls: number;
  modules: ReturnType<typeof makeModuleRow>[];
}

let harness: Harness;

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

beforeEach(() => {
  hostReset();
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
    modulesCalls: 0,
    modules: [makeModuleRow()],
  };
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(`/api/workspaces/acme/projects/${MODULES_PROJECT_ID}/`)) {
      if (harness.projectDelayMs > 0) await sleep(harness.projectDelayMs);
      return jsonResponse(url, harness.projectStatus, harness.project);
    }
    if (url.endsWith("/project-roles/")) return jsonResponse(url, 200, harness.roles);
    if (url.endsWith(`/api/workspaces/acme/projects/${MODULES_PROJECT_ID}/modules/`)) {
      harness.modulesCalls += 1;
      if (harness.modulesDelayMs > 0) await sleep(harness.modulesDelayMs);
      return jsonResponse(url, harness.modulesStatus, harness.modules);
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
  document.title = "";
});

function renderScreen(children?: React.ReactNode): ReturnType<typeof render> {
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ModulesListScreen workspaceSlug="acme" projectId={MODULES_PROJECT_ID}>
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

  it("falls back to the crumb as the title before the project arrives", async () => {
    harness.projectDelayMs = 50;
    renderScreen();
    await waitFor(() => expect(document.title).toBe("Modules"));
    await waitFor(() => expect(document.title).toBe("Website relaunch - Modules"));
  });
});
