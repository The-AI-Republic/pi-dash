// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Estimates settings surface (PSET-049, EST-001–007/013/016): the loader,
// the illustrated empty state and its create action, the active row with
// delete-only controls, superseded rows with none, and the enable switch
// that writes the active id or clears it.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { authKeys } from "../auth/index.js";
import { notifyError, notifySuccess } from "../../shared/shell/index.js";
import { ESTIMATE_CREATED_TITLE, ESTIMATE_CREATE_FAILED_TITLE, ESTIMATE_DELETED_TITLE } from "./api.js";
import { ESTIMATE_DIALOG_TITLE } from "./EstimateCreateDialog.js";
import { UPDATE_DIALOG_SAVE_LABEL, UPDATE_DIALOG_TITLE } from "./EstimateUpdateDialog.js";
import { ESTIMATE_SAVED_EDIT_FLAG } from "./model.js";
import {
  ESTIMATES_ADD_LABEL,
  ESTIMATES_ARCHIVED_HEADING,
  ESTIMATES_EMPTY_TITLE,
  ESTIMATES_RETRY_LABEL,
  EstimatesScreen,
  ESTIMATE_DELETE_DIALOG_TITLE,
} from "./EstimatesScreen.js";

const WORKSPACE = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ACTIVE_ID = "44444444-4444-4444-8444-444444444444";
const OLD_ID = "55555555-5555-4555-8555-555555555555";

vi.mock("../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
  notifyInfo: vi.fn(),
}));

const editionHarness = vi.hoisted(() => ({ flags: {} as Record<string, boolean> }));
vi.mock("../../core/edition/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../core/edition/index.js")>();
  return {
    ...original,
    get edition() {
      return { ...original.edition, flags: editionHarness.flags };
    },
  };
});

function jsonResponse(url: string, status: number, body: unknown): TransportResponse {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) } as TransportResponse;
}

interface Row {
  id: string;
  name: string;
  type: string;
  last_used: boolean;
  created_at: string;
  points: { id: string; key: number; value: string; description: string }[];
}

function systemRow(overrides: Partial<Row> & { id: string; name: string }): Row {
  return {
    type: "points",
    last_used: false,
    created_at: "2024-01-01T00:00:00Z",
    points: [
      { id: "aaaaaaaa-0001-4000-8000-000000000001", key: 1, value: "1", description: "" },
      { id: "aaaaaaaa-0002-4000-8000-000000000002", key: 2, value: "2", description: "" },
    ],
    ...overrides,
  };
}

function wireRow(row: Row): Record<string, unknown> {
  return {
    workspace: "11111111-1111-4111-8111-111111111111",
    project: PROJECT,
    description: "",
    updated_at: row.created_at,
    created_by: null,
    updated_by: null,
    ...row,
  };
}

function projectRow(estimate: string | null): Record<string, unknown> {
  const base = {
    id: PROJECT,
    name: "Website",
    identifier: "WEB",
    logo_props: {},
    estimate,
  };
  // The detail contract is strict; the switch's PATCH parses the full row.
  return {
    ...base,
    created_at: "2024-01-01T00:00:00Z",
    updated_at: "2024-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: "11111111-1111-4111-8111-111111111111",
    description: "",
    description_text: null,
    description_html: null,
    network: 2,
    default_assignee: null,
    project_lead: null,
    emoji: null,
    icon_prop: null,
    module_view: false,
    cycle_view: false,
    issue_views_view: false,
    page_view: false,
    intake_view: false,
    inbox_view: false,
    is_time_tracking_enabled: false,
    is_issue_type_enabled: true,
    is_default: false,
    guest_view_all_features: false,
    members_can_edit_states: true,
    cover_image: null,
    cover_image_asset: null,
    cover_image_url: null,
    archive_in: 0,
    close_in: 0,
    default_state: null,
    archived_at: null,
    timezone: "UTC",
    external_source: null,
    external_id: null,
    repo_url: "",
    base_branch: "",
    is_favorite: false,
    sort_order: null,
    member_role: 20,
    anchor: null,
    members: [],
    next_work_item_sequence: 1,
    agent_executor_options: [],
    default_agent_executor: "",
  };
}

interface StubOptions {
  systems?: Row[];
  estimate?: string | null;
  holdList?: Promise<void>;
  listStatus?: number;
  createStatus?: number;
  createBody?: unknown;
  deleteStatus?: number;
  patchStatus?: number;
}

function stubApi(options: StubOptions = {}) {
  const calls: { method: string; url: string; body: unknown }[] = [];
  let systems = options.systems ?? [];
  let projectEstimate = options.estimate ?? null;
  const estimatesUrl = `/api/workspaces/${WORKSPACE}/projects/${PROJECT}/estimates/`;
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    const body = init.body === undefined ? undefined : (JSON.parse(init.body) as unknown);
    calls.push({ method, url, body });
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(estimatesUrl) && method === "GET") {
      if (options.holdList !== undefined) await options.holdList;
      if ((options.listStatus ?? 200) !== 200) return jsonResponse(url, options.listStatus ?? 500, {});
      return jsonResponse(url, 200, systems.map(wireRow));
    }
    if (url.endsWith(estimatesUrl) && method === "POST") {
      if ((options.createStatus ?? 200) !== 200)
        return jsonResponse(url, options.createStatus ?? 500, options.createBody ?? {});
      const input = body as {
        estimate: { name: string; type: string; last_used: boolean };
        estimate_points: unknown[];
      };
      const created = systemRow({
        id: "99999999-9999-4999-8999-999999999999",
        name: input.estimate.name,
        type: input.estimate.type,
        last_used: input.estimate.last_used,
      });
      systems = [...systems, created];
      projectEstimate = null;
      return jsonResponse(url, 200, wireRow(created));
    }
    if (url.includes("/estimate-points/") && method === "POST") {
      const input = body as { key: number; value: string };
      return jsonResponse(url, 200, {
        id: `99999999-9999-4999-8999-99999999990${input.key}`,
        key: input.key,
        value: input.value,
        description: "",
      });
    }
    if (url.includes("/estimate-points/") && method === "DELETE") return jsonResponse(url, 200, []);
    if (url.includes(estimatesUrl) && !url.endsWith(estimatesUrl) && method === "PATCH") {
      const id = url.split("/estimates/")[1]?.replace(/\/$/, "") ?? "";
      const input = body as {
        estimate?: { name?: string };
        estimate_points: { id: string; key: number; value: string }[];
      };
      systems = systems.map((row) =>
        row.id === id
          ? {
              ...row,
              name: input.estimate?.name ?? row.name,
              points: input.estimate_points.map((entry) => ({ ...entry, description: "" })),
            }
          : row
      );
      const updated = systems.find((row) => row.id === id) as Row;
      return jsonResponse(url, 200, wireRow(updated));
    }
    if (url.includes(`/api/workspaces/${WORKSPACE}/projects/${PROJECT}/estimates/`) && method === "DELETE") {
      if ((options.deleteStatus ?? 204) !== 204) return jsonResponse(url, options.deleteStatus ?? 500, {});
      const id = url.split("/estimates/")[1]?.replace(/\/$/, "") ?? "";
      systems = systems.filter((row) => row.id !== id);
      return { ...jsonResponse(url, 204, {}), text: async () => "" } as TransportResponse;
    }
    if (url.endsWith(`/api/workspaces/${WORKSPACE}/projects/${PROJECT}/`) && method === "PATCH") {
      if ((options.patchStatus ?? 200) !== 200) return jsonResponse(url, options.patchStatus ?? 500, {});
      projectEstimate = (body as { estimate?: string | null }).estimate ?? null;
      return jsonResponse(url, 200, projectRow(projectEstimate));
    }
    if (url.endsWith(`/api/workspaces/${WORKSPACE}/projects/${PROJECT}/`) && method === "GET") {
      return jsonResponse(url, 200, projectRow(projectEstimate));
    }
    throw new Error(`unexpected fetch ${method} ${url}`);
  }) as never);
  return { calls };
}

function renderScreen(
  canManage = true,
  estimate: string | null = null,
  queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
) {
  queryClient.setQueryData(authKeys.project(WORKSPACE, PROJECT), projectRow(estimate));
  const view = render(
    <QueryClientProvider client={queryClient}>
      <EstimatesScreen workspaceSlug={WORKSPACE} projectId={PROJECT} canManage={canManage} />
    </QueryClientProvider>
  );
  return { queryClient, view };
}

beforeEach(() => {
  editionHarness.flags = {};
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("EstimatesScreen", () => {
  it("shows its loader while the list read is held", async () => {
    let release: () => void = () => undefined;
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    stubApi({ holdList: held });
    renderScreen();
    const loader = screen.getByRole("status", { name: "Loading estimates" });
    expect(loader).toBeInTheDocument();
    // Row-shaped skeleton set, not a spinner: four skeleton rows.
    expect(loader.querySelectorAll("span")).toHaveLength(4);
    expect(screen.queryByText(ESTIMATES_EMPTY_TITLE)).not.toBeInTheDocument();
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    release();
    await screen.findByText(ESTIMATES_EMPTY_TITLE);
    expect(screen.queryByRole("status", { name: "Loading estimates" })).not.toBeInTheDocument();
  });

  it("offers creation from the empty state and creates the active system", async () => {
    const user = userEvent.setup();
    const { calls } = stubApi({ systems: [] });
    renderScreen();
    expect(await screen.findByText(ESTIMATES_EMPTY_TITLE)).toBeInTheDocument();
    expect(screen.queryByTestId("estimate-system-row")).not.toBeInTheDocument();
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: ESTIMATES_ADD_LABEL }));
    await user.click(await screen.findByRole("button", { name: /custom/i }));
    await user.click(screen.getByRole("button", { name: "Create estimate" }));

    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(ESTIMATE_CREATED_TITLE, expect.any(String)));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    const created = calls.find((call) => call.method === "POST");
    expect(created?.body).toEqual({
      estimate: { name: "Points", type: "points", last_used: true },
      estimate_points: [
        { key: 1, value: "1" },
        { key: 2, value: "2" },
      ],
    });
    await waitFor(() => expect(screen.getByTestId("estimate-system-row")).toHaveTextContent("Points"));
  });

  it("renders the active row with a delete control and superseded rows without any", async () => {
    stubApi({
      systems: [
        systemRow({ id: ACTIVE_ID, name: "Active system", last_used: true }),
        systemRow({ id: OLD_ID, name: "Old system", created_at: "2023-01-01T00:00:00Z" }),
      ],
    });
    renderScreen();
    await screen.findByText("Active system");
    const rows = screen.getAllByTestId("estimate-system-row");
    expect(rows).toHaveLength(2);
    expect(within(rows[0] as HTMLElement).getAllByRole("button")).toHaveLength(1);
    expect(within(rows[0] as HTMLElement).getByRole("button")).toHaveAccessibleName("Delete Active system");
    expect(within(rows[0] as HTMLElement).queryByRole("button", { name: /edit/i })).not.toBeInTheDocument();
    expect(screen.getByText(ESTIMATES_ARCHIVED_HEADING)).toBeInTheDocument();
    const archived = screen.getByTestId("estimates-archived-section");
    expect(within(archived).getByText(/older versions/)).toBeInTheDocument();
    const reference = within(archived).getByRole("link", { name: "Learn more about estimates" });
    expect(reference).toHaveAttribute("href", "https://github.com/The-AI-Republic/pi-dash#readme");
    expect(reference).toHaveAttribute("target", "_blank");
    expect(reference).toHaveAttribute("rel", "noreferrer");
    expect(within(rows[1] as HTMLElement).queryByRole("button")).not.toBeInTheDocument();
  });

  it("humanizes duration-family values on the active row, never raw minutes", async () => {
    stubApi({
      systems: [
        systemRow({
          id: ACTIVE_ID,
          name: "Clock system",
          type: "time",
          last_used: true,
          points: [
            { id: "aaaaaaaa-0002-4000-8000-000000000002", key: 2, value: "150", description: "" },
            { id: "aaaaaaaa-0001-4000-8000-000000000001", key: 1, value: "90", description: "" },
          ],
        }),
      ],
    });
    renderScreen();
    const row = await screen.findByTestId("estimate-system-row");
    expect(row).toHaveTextContent("Clock system");
    const text = row.textContent ?? "";
    expect(text).toContain("1h 30m");
    expect(text).toContain("2h 30m");
    expect(text.indexOf("1h 30m")).toBeLessThan(text.indexOf("2h 30m"));
    expect(text).not.toContain("90");
    expect(text).not.toContain("150");
  });

  it("keeps the explainer and reference inside the archived section only", async () => {
    stubApi({ systems: [systemRow({ id: ACTIVE_ID, name: "Active system", last_used: true })] });
    renderScreen();
    await screen.findByText("Active system");
    expect(screen.queryByTestId("estimates-archived-section")).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Learn more about estimates" })).not.toBeInTheDocument();
  });

  it("writes the active id on enable and null on disable with notices", async () => {
    const user = userEvent.setup();
    const { calls } = stubApi({
      systems: [systemRow({ id: ACTIVE_ID, name: "Active system", last_used: true })],
      estimate: null,
    });
    renderScreen();
    await screen.findByText("Active system");
    const control = screen.getByRole("switch", { name: /enable estimates/i });
    expect(control).toHaveAttribute("aria-checked", "false");
    await user.click(control);
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Estimates enabled", expect.any(String)));
    expect(calls.filter((call) => call.method === "PATCH").map((call) => call.body)).toEqual([{ estimate: ACTIVE_ID }]);
    await user.click(control);
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Estimates disabled", expect.any(String)));
    expect(calls.filter((call) => call.method === "PATCH").map((call) => call.body)).toEqual([
      { estimate: ACTIVE_ID },
      { estimate: null },
    ]);
  });

  it("reads the switch from the stored reference and links the active id regardless of list order", async () => {
    const user = userEvent.setup();
    const { calls } = stubApi({
      systems: [
        systemRow({ id: OLD_ID, name: "Old system", created_at: "2024-06-01T00:00:00Z" }),
        systemRow({ id: ACTIVE_ID, name: "Active system", last_used: true }),
      ],
      estimate: null,
    });
    renderScreen();
    await screen.findByText("Active system");
    const control = screen.getByRole("switch", { name: /enable estimates/i });
    // The project stores no reference even though a system is marked
    // active, so the switch reads off.
    expect(control).toHaveAttribute("aria-checked", "false");
    await user.click(control);
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Estimates enabled", expect.any(String)));
    // Enabling links the active system's id, not the first-listed row.
    expect(calls.filter((call) => call.method === "PATCH").map((call) => call.body)).toEqual([{ estimate: ACTIVE_ID }]);
  });

  it("keeps the switch position and notices a refused toggle", async () => {
    const user = userEvent.setup();
    stubApi({
      systems: [systemRow({ id: ACTIVE_ID, name: "Active system", last_used: true })],
      estimate: ACTIVE_ID,
      patchStatus: 500,
    });
    renderScreen(true, ACTIVE_ID);
    await screen.findByText("Active system");
    const control = screen.getByRole("switch", { name: /enable estimates/i });
    expect(control).toHaveAttribute("aria-checked", "true");
    await user.click(control);
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn’t change estimates", expect.any(String)));
    expect(control).toHaveAttribute("aria-checked", "true");
  });

  it("hides every action from a non-admin viewer", async () => {
    stubApi({ systems: [systemRow({ id: ACTIVE_ID, name: "Active system", last_used: true })] });
    renderScreen(false);
    await screen.findByText("Active system");
    expect(screen.queryByRole("button", { name: /delete/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: ESTIMATES_ADD_LABEL })).not.toBeInTheDocument();
    expect(screen.getByRole("switch", { name: /enable estimates/i })).toHaveAttribute("aria-disabled", "true");
  });

  it("deleting the active system confirms, then clears the project reference", async () => {
    const user = userEvent.setup();
    const { calls } = stubApi({
      systems: [systemRow({ id: ACTIVE_ID, name: "Active system", last_used: true })],
      estimate: ACTIVE_ID,
    });
    renderScreen(true, ACTIVE_ID);
    await screen.findByText("Active system");
    // Creation belongs to the empty state alone; a populated screen has
    // no add control beside its rows.
    expect(screen.queryByRole("button", { name: ESTIMATES_ADD_LABEL })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Delete Active system" }));
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent("Delete estimate system");
    expect(dialog).toHaveTextContent("Active system");
    expect(dialog).toHaveTextContent("permanently");
    expect(dialog).toHaveTextContent("This cannot be undone");

    // Cancel keeps everything.
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(screen.getByText("Active system")).toBeInTheDocument();

    // Escape dismisses the confirmation while nothing is in flight.
    await user.click(screen.getByRole("button", { name: "Delete Active system" }));
    expect(await screen.findByRole("dialog")).toHaveTextContent(ESTIMATE_DELETE_DIALOG_TITLE);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());

    await user.click(screen.getByRole("button", { name: "Delete Active system" }));
    await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(ESTIMATE_DELETED_TITLE, expect.any(String)));
    expect(calls.filter((call) => call.method === "DELETE")).toHaveLength(1);
    expect(calls).toContainEqual(expect.objectContaining({ method: "PATCH", body: { estimate: null } }));
    await waitFor(() => expect(screen.queryByTestId("estimate-system-row")).not.toBeInTheDocument());
  });

  it("deleting a superseded system leaves the project reference alone", async () => {
    stubApi({
      systems: [
        systemRow({ id: ACTIVE_ID, name: "Active system", last_used: true }),
        systemRow({ id: OLD_ID, name: "Old system", created_at: "2023-01-01T00:00:00Z" }),
      ],
      estimate: ACTIVE_ID,
    });
    renderScreen();
    await screen.findByText("Old system");
    expect(screen.queryByRole("button", { name: "Delete Old system" })).not.toBeInTheDocument();
  });

  it("shows a loader error instead of the empty state when the read fails", async () => {
    const user = userEvent.setup();
    const api = { listStatus: 500 };
    stubApi(api);
    renderScreen();
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Estimates could not be loaded");
    // A failure never reads as "no systems": no empty state, no rows,
    // no switch — just the error and its retry.
    expect(screen.queryByText(ESTIMATES_EMPTY_TITLE)).not.toBeInTheDocument();
    expect(screen.queryByTestId("estimate-system-row")).not.toBeInTheDocument();
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    expect(screen.queryByTestId("estimates-screen")).not.toBeInTheDocument();

    api.listStatus = 200;
    await user.click(screen.getByRole("button", { name: ESTIMATES_RETRY_LABEL }));
    await screen.findByText(ESTIMATES_EMPTY_TITLE);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("opens the creation dialog on step one from the empty-state action", async () => {
    const user = userEvent.setup();
    stubApi({ systems: [] });
    renderScreen();
    await user.click(await screen.findByRole("button", { name: ESTIMATES_ADD_LABEL }));
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent(ESTIMATE_DIALOG_TITLE);
  });

  it("pins the absences: no export, filter, search or bulk controls", async () => {
    stubApi({
      systems: [
        systemRow({ id: ACTIVE_ID, name: "Active system", last_used: true }),
        systemRow({ id: OLD_ID, name: "Old system", created_at: "2023-01-01T00:00:00Z" }),
      ],
    });
    renderScreen();
    await screen.findByText("Old system");
    const surface = screen.getByTestId("estimates-screen");
    expect(within(surface).queryByRole("searchbox")).not.toBeInTheDocument();
    expect(within(surface).queryByRole("checkbox")).not.toBeInTheDocument();
    expect(within(surface).queryByRole("button", { name: /export|import|download/i })).not.toBeInTheDocument();
    expect(within(surface).queryByRole("button", { name: /filter|search/i })).not.toBeInTheDocument();
    expect(within(surface).queryByRole("button", { name: /bulk|select all/i })).not.toBeInTheDocument();
    expect(within(surface).queryByRole("menuitem", { name: /export|import|download/i })).not.toBeInTheDocument();
    expect(surface.querySelector('input[type="search"]')).toBeNull();
  });

  it("keeps the absent behaviors out of the populated screen text", async () => {
    stubApi({
      systems: [
        systemRow({ id: ACTIVE_ID, name: "Active system", last_used: true }),
        systemRow({ id: OLD_ID, name: "Old system", created_at: "2023-01-01T00:00:00Z" }),
      ],
    });
    renderScreen();
    await screen.findByText("Old system");
    const text = screen.getByTestId("estimates-screen").textContent ?? "";
    for (const word of ["shortcut", "export", "import", "bulk", "filter", "search"]) {
      expect(text).not.toMatch(new RegExp(word, "i"));
    }
  });

  it("keeps the absent behaviors out of the empty screen text", async () => {
    stubApi({});
    renderScreen();
    await screen.findByText(ESTIMATES_EMPTY_TITLE);
    const text = screen.getByTestId("estimates-screen").textContent ?? "";
    for (const word of ["shortcut", "export", "import", "bulk", "filter", "search"]) {
      expect(text).not.toMatch(new RegExp(word, "i"));
    }
  });

  it("ignores shortcut keys on the screen", async () => {
    const user = userEvent.setup();
    stubApi({ systems: [systemRow({ id: ACTIVE_ID, name: "Active system", last_used: true })] });
    renderScreen();
    await screen.findByText("Active system");
    await user.keyboard("?");
    await user.keyboard("c");
    await user.keyboard("/");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Estimates" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("hides the empty-state action from a non-admin viewer", async () => {
    stubApi({});
    renderScreen(false);
    await screen.findByText(ESTIMATES_EMPTY_TITLE);
    expect(screen.queryByRole("button", { name: ESTIMATES_ADD_LABEL })).not.toBeInTheDocument();
  });

  it("keeps the create dialog open with a notice when the write fails", async () => {
    const user = userEvent.setup();
    stubApi({ createStatus: 500 });
    renderScreen();
    await screen.findByText(ESTIMATES_EMPTY_TITLE);
    await user.click(screen.getByRole("button", { name: ESTIMATES_ADD_LABEL }));
    await user.click(await screen.findByRole("button", { name: /custom/i }));
    await user.click(screen.getByRole("button", { name: "Create estimate" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("refuses a duplicate name with a failure notice and keeps the dialog open", async () => {
    const user = userEvent.setup();
    stubApi({ createStatus: 400, createBody: { error: "The payload is not valid" } });
    renderScreen();
    await screen.findByText(ESTIMATES_EMPTY_TITLE);
    await user.click(screen.getByRole("button", { name: ESTIMATES_ADD_LABEL }));
    await user.click(await screen.findByRole("button", { name: /custom/i }));
    await user.click(screen.getByRole("button", { name: "Create estimate" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(ESTIMATE_CREATE_FAILED_TITLE, expect.any(String)));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.queryByTestId("estimate-system-row")).not.toBeInTheDocument();
  });

  it("skips the project write when deleting the active system while unlinked", async () => {
    const user = userEvent.setup();
    const { calls } = stubApi({
      systems: [systemRow({ id: ACTIVE_ID, name: "Active system", last_used: true })],
      estimate: null,
    });
    renderScreen();
    await screen.findByText("Active system");
    await user.click(screen.getByRole("button", { name: "Delete Active system" }));
    await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(ESTIMATE_DELETED_TITLE, expect.any(String)));
    expect(calls.filter((call) => call.method === "DELETE")).toHaveLength(1);
    expect(calls.filter((call) => call.method === "PATCH")).toHaveLength(0);
  });

  it("keeps the unrelated link when deleting the active system while linked elsewhere", async () => {
    // Narrow unlink rule (NEWFRONT-809): the flagged row is deleted but
    // the reference points at the older system, so no project write runs
    // and estimates stay on.
    const user = userEvent.setup();
    const { calls } = stubApi({
      systems: [
        systemRow({ id: ACTIVE_ID, name: "Active system", last_used: true }),
        systemRow({ id: OLD_ID, name: "Old system", created_at: "2023-01-01T00:00:00Z" }),
      ],
      estimate: OLD_ID,
    });
    const { queryClient } = renderScreen(true, OLD_ID);
    await screen.findByText("Active system");
    await user.click(screen.getByRole("button", { name: "Delete Active system" }));
    await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(ESTIMATE_DELETED_TITLE, expect.any(String)));
    expect(calls.filter((call) => call.method === "DELETE")).toHaveLength(1);
    expect(calls.filter((call) => call.method === "PATCH")).toHaveLength(0);
    expect(queryClient.getQueryData(authKeys.project(WORKSPACE, PROJECT))).toMatchObject({ estimate: OLD_ID });
  });

  it("shows a generated name as-is and links it like any other system", async () => {
    const user = userEvent.setup();
    const { calls } = stubApi({
      systems: [systemRow({ id: ACTIVE_ID, name: "xkqzmpvtyr", last_used: true })],
      estimate: null,
    });
    renderScreen();
    await screen.findByText("xkqzmpvtyr");
    await user.click(screen.getByRole("switch", { name: /enable estimates/i }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Estimates enabled", expect.any(String)));
    expect(calls.filter((call) => call.method === "PATCH").map((call) => call.body)).toEqual([{ estimate: ACTIVE_ID }]);
  });

  it("offers the edit control beside delete when the edition allows saved edits", async () => {
    editionHarness.flags = { [ESTIMATE_SAVED_EDIT_FLAG]: true };
    stubApi({
      systems: [
        systemRow({ id: ACTIVE_ID, name: "Active system", last_used: true }),
        systemRow({ id: OLD_ID, name: "Old system", created_at: "2023-01-01T00:00:00Z" }),
      ],
    });
    renderScreen();
    await screen.findByText("Active system");
    const rows = screen.getAllByTestId("estimate-system-row");
    const controls = within(rows[0] as HTMLElement).getAllByRole("button");
    expect(controls).toHaveLength(2);
    expect(controls[0]).toHaveAccessibleName("Delete Active system");
    expect(controls[1]).toHaveAccessibleName("Edit Active system");
    expect(within(rows[1] as HTMLElement).queryByRole("button")).not.toBeInTheDocument();
  });

  it("hides the edit control from a non-admin viewer even in the cloud edition", async () => {
    editionHarness.flags = { [ESTIMATE_SAVED_EDIT_FLAG]: true };
    stubApi({ systems: [systemRow({ id: ACTIVE_ID, name: "Active system", last_used: true })] });
    renderScreen(false);
    await screen.findByText("Active system");
    expect(screen.queryByRole("button", { name: /edit/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /delete/i })).not.toBeInTheDocument();
  });

  it("opens the update dialog from the row and saves a rename through it", async () => {
    editionHarness.flags = { [ESTIMATE_SAVED_EDIT_FLAG]: true };
    const user = userEvent.setup();
    const { calls } = stubApi({
      systems: [systemRow({ id: ACTIVE_ID, name: "Active system", last_used: true })],
    });
    renderScreen();
    await screen.findByText("Active system");
    await user.click(screen.getByRole("button", { name: "Edit Active system" }));
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent(UPDATE_DIALOG_TITLE);
    const name = within(dialog).getByRole("textbox", { name: "System name" });
    await user.clear(name);
    await user.type(name, "Renamed system");
    await user.click(within(dialog).getByRole("button", { name: UPDATE_DIALOG_SAVE_LABEL }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Estimate updated", expect.any(String)));
    expect(calls.find((call) => call.method === "PATCH")?.body).toEqual({
      estimate: { name: "Renamed system" },
      estimate_points: [
        { id: "aaaaaaaa-0001-4000-8000-000000000001", key: 1, value: "1" },
        { id: "aaaaaaaa-0002-4000-8000-000000000002", key: 2, value: "2" },
      ],
    });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    await screen.findByText("Renamed system");
  });
});
