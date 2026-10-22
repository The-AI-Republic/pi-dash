// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Views palette wiring (parity/specs/views.md §12, VIEW-042): gate
// matrix, dialog opening, jump navigation, and cleanup.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Stand-in for the router: navigation is TanStack's own tested behavior.
// What this suite owns is the registration and its reads.
const routerHarness = vi.hoisted(() => ({ navigate: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => routerHarness.navigate,
}));

// The lazy create dialog reads the session; the entries never do, so
// these stand-ins only serve the dialog-opening test below.
const sessionHarness = vi.hoisted(() => ({ useMe: vi.fn(), usePermissions: vi.fn() }));

vi.mock("../../../core/session/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../core/session/index.js")>()),
  useMe: sessionHarness.useMe,
  usePermissions: sessionHarness.usePermissions,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  clearCommands,
  clearPalettePages,
  findCommand,
  findCommandBySequence,
  findPalettePage,
  useCommandPalette,
  type PalettePage,
} from "../../../shared/commands/index.js";
import { jsonResponse, makeMember, makeView, VIEWS_OWNER_ID } from "../testing.js";
import { VIEW_CREATE_COMMAND_ID, VIEW_JUMP_COMMAND_ID, VIEW_JUMP_PAGE_ID } from "./viewCommands.js";
import { ViewsPaletteEntries } from "./ViewsPaletteEntries.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const WS_ID = "123e4567-e89b-12d3-a456-426614174000";

function projectRow(overrides: Record<string, unknown> = {}) {
  return {
    id: PROJECT_ID,
    name: "Website relaunch",
    identifier: "WEB",
    sort_order: null,
    logo_props: {},
    member_role: 15,
    archived_at: null,
    workspace: WS_ID,
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
    agent_executor_options: [],
    default_agent_executor: "local_runner",
    ...overrides,
  };
}

const harness = vi.hoisted(
  (): {
    project: Record<string, unknown>;
    projectDelayMs: number;
    roles: unknown;
    rolesStatus: number;
    views: ReturnType<typeof makeView>[];
  } => ({ project: {}, projectDelayMs: 0, roles: {}, rolesStatus: 200, views: [] })
);

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function renderEntries(): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ViewsPaletteEntries workspaceSlug="acme" projectId={PROJECT_ID} />
    </QueryClientProvider>
  );
}

function PageHarness({ page }: { page: PalettePage }): React.ReactElement {
  const state = page.usePageState("");
  return (
    <div>
      {state.options.map((option) => (
        <button
          key={option.id}
          type="button"
          onClick={() => {
            void option.run();
          }}
        >
          {option.title}
        </button>
      ))}
    </div>
  );
}

beforeEach(() => {
  harness.project = projectRow();
  harness.projectDelayMs = 0;
  harness.roles = { [PROJECT_ID]: 15 };
  harness.rolesStatus = 200;
  harness.views = [];
  routerHarness.navigate.mockReset();
  sessionHarness.useMe.mockReturnValue({ data: { id: VIEWS_OWNER_ID } });
  sessionHarness.usePermissions.mockReturnValue({ isAdmin: false, isLoading: false });
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(`/api/workspaces/acme/projects/${PROJECT_ID}/`)) {
      if (harness.projectDelayMs > 0) await sleep(harness.projectDelayMs);
      return jsonResponse(url, 200, harness.project);
    }
    if (url.endsWith("/project-roles/")) return jsonResponse(url, harness.rolesStatus, harness.roles);
    if (url.endsWith(`/api/workspaces/acme/projects/${PROJECT_ID}/views/`)) {
      return jsonResponse(url, 200, harness.views);
    }
    if (url.endsWith("/api/workspaces/acme/members/")) {
      return jsonResponse(url, 200, [makeMember(VIEWS_OWNER_ID)]);
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  clearCommands();
  clearPalettePages();
  useCommandPalette.getState().closePalette();
});

describe("ViewsPaletteEntries", () => {
  it("registers creation and jump for a member with the switch on", async () => {
    renderEntries();
    await waitFor(() => expect(findCommand(VIEW_CREATE_COMMAND_ID)).toBeDefined());
    const create = findCommand(VIEW_CREATE_COMMAND_ID);
    expect(create).toMatchObject({ title: "New view", section: "Create", sequence: "nv", hint: "Website relaunch" });
    const jump = findCommand(VIEW_JUMP_COMMAND_ID);
    expect(jump).toMatchObject({
      title: "Open a project view",
      section: "Navigate",
      sequence: "ov",
      page: VIEW_JUMP_PAGE_ID,
      hint: "Website relaunch",
    });
    expect(findCommandBySequence("nv")?.id).toBe(VIEW_CREATE_COMMAND_ID);
    expect(findCommandBySequence("ov")?.id).toBe(VIEW_JUMP_COMMAND_ID);
    expect(findPalettePage(VIEW_JUMP_PAGE_ID)).toBeDefined();
  });

  it("registers creation for an admin too", async () => {
    harness.roles = { [PROJECT_ID]: 20 };
    renderEntries();
    await waitFor(() => expect(findCommand(VIEW_CREATE_COMMAND_ID)).toBeDefined());
    expect(findCommand(VIEW_JUMP_COMMAND_ID)).toBeDefined();
  });

  it("registers only the jump menu for a guest with the switch on", async () => {
    harness.roles = { [PROJECT_ID]: 5 };
    harness.project = projectRow({ cycle_view: false });
    renderEntries();
    await waitFor(() => expect(findCommand(VIEW_JUMP_COMMAND_ID)).toBeDefined());
    expect(findCommand(VIEW_CREATE_COMMAND_ID)).toBeUndefined();
    expect(findCommandBySequence("nv")).toBeUndefined();
    expect(findCommandBySequence("ov")?.id).toBe(VIEW_JUMP_COMMAND_ID);
  });

  it("fails creation closed when the project standing is missing", async () => {
    harness.roles = {};
    renderEntries();
    await waitFor(() => expect(findCommand(VIEW_JUMP_COMMAND_ID)).toBeDefined());
    expect(findCommand(VIEW_CREATE_COMMAND_ID)).toBeUndefined();
  });

  it("fails creation closed when the standing read errors", async () => {
    harness.rolesStatus = 500;
    harness.roles = { detail: "boom" };
    renderEntries();
    await waitFor(() => expect(findCommand(VIEW_JUMP_COMMAND_ID)).toBeDefined());
    expect(findCommand(VIEW_CREATE_COMMAND_ID)).toBeUndefined();
  });

  it("registers neither entry while the switch is off", async () => {
    harness.project = projectRow({ issue_views_view: false, module_view: false });
    harness.roles = { [PROJECT_ID]: 20 };
    renderEntries();
    await waitFor(() => expect(findPalettePage(VIEW_JUMP_PAGE_ID)).toBeDefined());
    // Settle: the project read landed (the page registered on the same
    // render), so absent commands are gated, not loading.
    expect(findCommand(VIEW_CREATE_COMMAND_ID)).toBeUndefined();
    expect(findCommand(VIEW_JUMP_COMMAND_ID)).toBeUndefined();
    expect(findCommandBySequence("nv")).toBeUndefined();
    expect(findCommandBySequence("ov")).toBeUndefined();
  });

  it("registers nothing while the project loads, then both once it lands", async () => {
    harness.projectDelayMs = 50;
    renderEntries();
    await sleep(10);
    expect(findCommand(VIEW_CREATE_COMMAND_ID)).toBeUndefined();
    expect(findCommand(VIEW_JUMP_COMMAND_ID)).toBeUndefined();
    await waitFor(() => expect(findCommand(VIEW_CREATE_COMMAND_ID)).toBeDefined());
    expect(findCommand(VIEW_JUMP_COMMAND_ID)).toBeDefined();
  });

  it("unregisters both entries and the page on unmount", async () => {
    const { unmount } = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <ViewsPaletteEntries workspaceSlug="acme" projectId={PROJECT_ID} />
      </QueryClientProvider>
    );
    await waitFor(() => expect(findCommand(VIEW_CREATE_COMMAND_ID)).toBeDefined());
    expect(findPalettePage(VIEW_JUMP_PAGE_ID)).toBeDefined();
    unmount();
    expect(findCommand(VIEW_CREATE_COMMAND_ID)).toBeUndefined();
    expect(findCommand(VIEW_JUMP_COMMAND_ID)).toBeUndefined();
    expect(findPalettePage(VIEW_JUMP_PAGE_ID)).toBeUndefined();
  });

  it("opens the views-owned create dialog when the command fires", async () => {
    renderEntries();
    await waitFor(() => expect(findCommand(VIEW_CREATE_COMMAND_ID)).toBeDefined());
    expect(screen.queryByRole("heading", { name: "Create View" })).not.toBeInTheDocument();
    await findCommand(VIEW_CREATE_COMMAND_ID)?.run();
    expect(await screen.findByRole("heading", { name: "Create View" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("heading", { name: "Create View" })).not.toBeInTheDocument());
  });

  it("navigates to the picked view's detail from the jump menu", async () => {
    harness.views = [makeView({ id: "223e4567-e89b-12d3-a456-426614174013", name: "Triage" })];
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <ViewsPaletteEntries workspaceSlug="acme" projectId={PROJECT_ID} />
      </QueryClientProvider>
    );
    await waitFor(() => expect(findCommand(VIEW_JUMP_COMMAND_ID)).toBeDefined());
    const page = findPalettePage(VIEW_JUMP_PAGE_ID);
    if (!page) throw new Error("jump page missing");
    render(
      <QueryClientProvider client={queryClient}>
        <PageHarness page={page} />
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByRole("button", { name: "Triage" })).toBeInTheDocument());
    useCommandPalette.getState().openPalette();
    fireEvent.click(screen.getByRole("button", { name: "Triage" }));
    expect(routerHarness.navigate).toHaveBeenCalledWith({
      to: `/acme/projects/${PROJECT_ID}/views/223e4567-e89b-12d3-a456-426614174013`,
    });
    expect(useCommandPalette.getState().open).toBe(false);
  });
});
