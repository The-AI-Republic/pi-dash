// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// States tab composition (parity/specs/project-settings-states.md,
// PSET-028–035): seven cards from the cached list, skeleton before
// content, reader/edit rights from the flag, the toggle's reach, and
// the notices wired through the mutation hooks.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ProjectDetails, State, TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../project-settings/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, useProjectSettings: vi.fn() };
});
vi.mock("../../auth/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, useProjectRow: vi.fn() };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { useProjectRow } from "../../auth/index.js";
import { useProjectSettings } from "../../project-settings/index.js";
import { stateKeys } from "../api.js";

import { WORKFLOW_GROUPS } from "../groups.js";
import { state as row, TEST_PROJECT, TEST_WORKSPACE, testRowId, stateList } from "../testFixtures.js";
import {
  DELETE_DEFAULT_REASON,
  DELETE_LAST_IN_GROUP_REASON,
  deleteBlockedReason,
  SKELETON_ROWS,
  StatesTab,
} from "./StatesTab.js";

/** A complete project row for the settings reads and flag writes. */
function projectDetail(overrides: Partial<ProjectDetails> = {}): ProjectDetails {
  return {
    id: TEST_PROJECT,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: TEST_WORKSPACE,
    name: "Website relaunch",
    description: "",
    description_text: null,
    description_html: null,
    network: 2,
    identifier: "WEB",
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
    is_issue_type_enabled: false,
    is_default: false,
    guest_view_all_features: false,
    members_can_edit_states: true,
    cover_image: null,
    cover_image_asset: null,
    cover_image_url: null,
    estimate: null,
    archive_in: 0,
    close_in: 0,
    logo_props: {},
    default_state: null,
    archived_at: null,
    timezone: "UTC",
    external_source: null,
    external_id: null,
    repo_url: "",
    base_branch: "",
    is_favorite: false,
    sort_order: null,
    member_role: 15,
    anchor: null,
    members: [],
    next_work_item_sequence: 1,
    agent_executor_options: [{ kind: "local_runner", available: true, reason_code: "" }],
    default_agent_executor: "local_runner",
    ...overrides,
  };
}

const useSettings = vi.mocked(useProjectSettings);
const useProject = vi.mocked(useProjectRow);

interface StubOptions {
  gate?: Promise<void>;
  patchOk?: boolean;
  deleteOk?: boolean;
  calls?: string[];
}

function response(url: string, status: number, body?: unknown): TransportResponse {
  return {
    status,
    url,
    headers: { get: () => null },
    text: async () => (body === undefined ? "" : JSON.stringify(body)),
  } as TransportResponse;
}

function defaultRows(): State[] {
  return [
    row({ name: "Backlog", group: "backlog", sequence: 1 }),
    row({ name: "Triage queue", group: "backlog", sequence: 2 }),
    row({ name: "Todo", group: "unstarted", sequence: 3, default: true }),
    row({ name: "In progress", group: "started", sequence: 4 }),
    row({ name: "Blocked", group: "started", sequence: 5 }),
    row({ name: "Review", group: "review", sequence: 4 }),
    row({ name: "Testing", group: "test", sequence: 5 }),
    row({ name: "Done", group: "completed", sequence: 6 }),
  ];
}

function stubApi(options: StubOptions = {}) {
  const calls = options.calls ?? [];
  createAppClient("https://api.test", (async (url: string, init?: { method?: string; body?: string }) => {
    const method = init?.method ?? "GET";
    calls.push(`${method} ${url}`);
    if (url.endsWith("/auth/get-csrf-token/")) return response(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(`/projects/${TEST_PROJECT}/states/`) && method === "GET") {
      if (options.gate) await options.gate;
      return response(url, 200, defaultRows());
    }
    if (url.endsWith("/states/") && method === "POST") {
      const input = JSON.parse(init?.body ?? "{}") as { name: string; group: State["group"]; color: string };
      return response(url, 200, row({ name: input.name, group: input.group, color: input.color }));
    }
    if (url.includes("/states/") && method === "PATCH") {
      if (options.patchOk === false) return response(url, 500, { detail: "refused" });
      return response(url, 200, row({ name: "Updated", group: "started" }));
    }
    if (url.includes("/states/") && method === "DELETE") {
      return options.deleteOk === false ? response(url, 400, { error: "State is in use" }) : response(url, 204);
    }
    if (url.endsWith("/mark-default/")) return response(url, 204);
    if (url.endsWith(`/projects/${TEST_PROJECT}/`) && method === "PATCH") return response(url, 200, projectDetail());
    throw new Error(`unexpected ${method} ${url}`);
  }) as never);
  return calls;
}

function setViewer(role: number | null, flag: boolean, projectPending = false) {
  useSettings.mockReturnValue({
    workspaceSlug: TEST_WORKSPACE,
    projectId: TEST_PROJECT,
    role,
    workspaceAdmin: false,
    project: null,
  });
  useProject.mockReturnValue({
    data: projectPending ? undefined : projectDetail({ members_can_edit_states: flag }),
    isPending: projectPending,
  } as never);
}

function renderTab(seed?: State[]) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  if (seed !== undefined) client.setQueryData(stateKeys.list(TEST_WORKSPACE, TEST_PROJECT), seed);
  render(
    <QueryClientProvider client={client}>
      <StatesTab />
    </QueryClientProvider>
  );
  return client;
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  cleanup();
  resetAppClient();
});

describe("deleteBlockedReason", () => {
  const rows = stateList([
    { name: "a", group: "backlog" },
    { name: "b", group: "backlog" },
  ]);

  it("protects the default state", () => {
    expect(deleteBlockedReason({ ...rows[0]!, default: true }, rows)).toBe(DELETE_DEFAULT_REASON);
  });

  it("protects the last state of a group", () => {
    expect(deleteBlockedReason(rows[0]!, [rows[0]!])).toBe(DELETE_LAST_IN_GROUP_REASON);
  });

  it("allows another row in a multi-row group", () => {
    expect(deleteBlockedReason(rows[0]!, rows)).toBeNull();
  });
});

describe("StatesTab loading and reading", () => {
  it("sketches a multi-row skeleton before grouped content while the read is pending", () => {
    setViewer(20, true);
    stubApi({ gate: new Promise<never>(() => undefined) });
    renderTab();
    const skeleton = screen.getByTestId("states-skeleton");
    expect(skeleton.children).toHaveLength(SKELETON_ROWS);
    expect(screen.queryByTestId("state-group-card")).not.toBeInTheDocument();
  });

  it("renders all seven cards once the read lands, with review folded", async () => {
    setViewer(20, true);
    stubApi();
    renderTab();
    await waitFor(() => {
      expect(screen.getAllByTestId("state-group-card")).toHaveLength(WORKFLOW_GROUPS.length);
    });
    const cards = screen.getAllByTestId("state-group-card");
    expect(cards.map((card) => card.getAttribute("data-group"))).toEqual([...WORKFLOW_GROUPS]);
    const review = cards.find((card) => card.getAttribute("data-group") === "review")!;
    expect(within(review).queryByTestId("state-group-body")).not.toBeInTheDocument();
    for (const card of cards.filter((entry) => entry !== review)) {
      expect(within(card).getByTestId("state-group-body")).toBeInTheDocument();
    }
  });

  it("opens a collapsed card through its header", async () => {
    const user = userEvent.setup();
    setViewer(20, true);
    stubApi();
    renderTab();
    const review = (await screen.findAllByTestId("state-group-card")).find(
      (card) => card.getAttribute("data-group") === "review"
    )!;
    expect(within(review).queryByTestId("state-group-body")).not.toBeInTheDocument();
    await user.click(within(review).getByTestId("state-group-toggle"));
    expect(within(review).getByTestId("state-group-body")).toBeInTheDocument();
    expect(
      within(review)
        .getAllByTestId("state-row")
        .map((node) => node.getAttribute("data-state-name"))
    ).toEqual(["Review"]);
  });

  it("shows the empty note for a group with no states", async () => {
    setViewer(20, true);
    stubApi();
    renderTab();
    const cancelled = (await screen.findAllByTestId("state-group-card")).find(
      (card) => card.getAttribute("data-group") === "cancelled"
    )!;
    expect(within(cancelled).getByTestId("state-group-empty").textContent).toContain(
      "No states available for the Cancelled group."
    );
    expect(within(cancelled).getByRole("button", { name: "Create a state" })).toBeInTheDocument();
  });

  it("closes the edit form through cancel without a write", async () => {
    const user = userEvent.setup();
    setViewer(20, true);
    const calls = stubApi();
    renderTab();
    const started = (await screen.findAllByTestId("state-group-card")).find(
      (card) => card.getAttribute("data-group") === "started"
    )!;
    await user.click(within(started).getByRole("button", { name: "Edit In progress" }));
    await user.click(within(started).getByRole("button", { name: "Cancel" }));
    expect(within(started).queryByTestId("state-form")).not.toBeInTheDocument();
    expect(calls.some((call) => call.startsWith("PATCH "))).toBe(false);
  });

  it("closes an open create form through cancel without a write", async () => {
    const user = userEvent.setup();
    setViewer(20, true);
    const calls = stubApi();
    renderTab();
    const started = (await screen.findAllByTestId("state-group-card")).find(
      (card) => card.getAttribute("data-group") === "started"
    )!;
    await user.click(within(started).getByRole("button", { name: "Add a state to Started" }));
    await user.click(within(started).getByRole("button", { name: "Cancel" }));
    expect(within(started).queryByTestId("state-form")).not.toBeInTheDocument();
    expect(calls.some((call) => call.startsWith("POST "))).toBe(false);
  });

  it("closes the delete dialog through cancel without a write", async () => {
    const user = userEvent.setup();
    setViewer(20, true);
    const calls = stubApi();
    renderTab();
    const started = (await screen.findAllByTestId("state-group-card")).find(
      (card) => card.getAttribute("data-group") === "started"
    )!;
    await user.click(within(started).getByRole("button", { name: "Delete In progress" }));
    await user.click(await screen.findByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
    expect(calls.some((call) => call.startsWith("DELETE "))).toBe(false);
  });

  it("disables the delete control with its explanation on the default row", async () => {
    setViewer(20, true);
    stubApi();
    renderTab();
    const unstarted = (await screen.findAllByTestId("state-group-card")).find(
      (card) => card.getAttribute("data-group") === "unstarted"
    )!;
    expect(within(unstarted).getByText(DELETE_DEFAULT_REASON)).toBeInTheDocument();
    expect(within(unstarted).getByRole("button", { name: /Delete Todo/ })).toBeDisabled();
  });

  it("gives a notice-only load failure whose retry re-reads the list", async () => {
    const user = userEvent.setup();
    setViewer(5, true);
    let attempts = 0;
    createAppClient("https://api.test", (async (url: string, init?: { method?: string }) => {
      if (url.endsWith("/auth/get-csrf-token/")) return response(url, 200, { csrf_token: "csrf" });
      if ((init?.method ?? "GET") === "GET") {
        attempts += 1;
        return attempts === 1 ? response(url, 500, { detail: "boom" }) : response(url, 200, defaultRows());
      }
      throw new Error("unexpected");
    }) as never);
    renderTab();
    const failed = await screen.findByTestId("states-load-failed");
    expect(failed).toHaveTextContent("couldn’t be loaded");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    await user.click(within(failed).getByRole("button", { name: "Try again" }));
    await waitFor(() => {
      expect(screen.getAllByTestId("state-group-card")).toHaveLength(WORKFLOW_GROUPS.length);
    });
    expect(attempts).toBeGreaterThanOrEqual(2);
  });
});

describe("rights", () => {
  it("lends admins the toggle and full editing even with the flag off", async () => {
    setViewer(20, false);
    stubApi();
    renderTab();
    expect(await screen.findByTestId("state-member-editing")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Let members edit states" })).not.toBeChecked();
    const cards = await screen.findAllByTestId("state-group-card");
    expect(within(cards[0]!).getByRole("button", { name: "Add a state to Backlog" })).toBeEnabled();
    expect(within(cards[0]!).getAllByTestId("state-row-grip")).toHaveLength(2);
  });

  it("gives members editing and no toggle while the flag is on", async () => {
    setViewer(15, true);
    stubApi();
    renderTab();
    const card = (await screen.findAllByTestId("state-group-card"))[0]!;
    expect(within(card).getAllByTestId("state-row-grip")[0]).toBeInTheDocument();
    expect(screen.queryByTestId("state-member-editing")).not.toBeInTheDocument();
  });

  it("gives members a reader view while the flag is off", async () => {
    setViewer(15, false);
    stubApi();
    renderTab();
    const card = (await screen.findAllByTestId("state-group-card"))[0]!;
    expect(within(card).queryByTestId("state-row-grip")).not.toBeInTheDocument();
    expect(within(card).queryByRole("button", { name: /Edit/ })).not.toBeInTheDocument();
    expect(within(card).getByRole("button", { name: "Add a state to Backlog" })).toBeDisabled();
    expect(screen.queryByTestId("state-member-editing")).not.toBeInTheDocument();
  });

  it("keeps the flag's default on while the project row loads", async () => {
    setViewer(15, false, true);
    stubApi();
    renderTab();
    const card = (await screen.findAllByTestId("state-group-card"))[0]!;
    expect(within(card).getAllByTestId("state-row-grip").length).toBeGreaterThan(0);
  });
});

describe("creation", () => {
  it("opens the form docked in the group, posts and closes on success", async () => {
    const user = userEvent.setup();
    setViewer(20, true);
    const calls = stubApi();
    renderTab();
    const backlog = (await screen.findAllByTestId("state-group-card"))[0]!;
    await user.click(within(backlog).getByRole("button", { name: "Add a state to Backlog" }));
    const form = within(backlog).getByRole("form", { name: "Create state in Backlog" });
    await user.type(within(form).getByLabelText("State name"), "Triage queue");
    await user.click(within(form).getByRole("button", { name: "Create state" }));
    await waitFor(() => {
      expect(calls.some((call) => call.startsWith("POST ") && call.endsWith("/states/"))).toBe(true);
    });
    await waitFor(() => {
      expect(screen.queryByTestId("state-form")).not.toBeInTheDocument();
    });
  });

  it("keeps the form open when the create is refused", async () => {
    const user = userEvent.setup();
    setViewer(20, true);
    createAppClient("https://api.test", (async (url: string, init?: { method?: string }) => {
      const method = init?.method ?? "GET";
      if (url.endsWith("/auth/get-csrf-token/")) return response(url, 200, { csrf_token: "csrf" });
      if (url.endsWith("/states/") && method === "GET") return response(url, 200, defaultRows());
      if (url.endsWith("/states/") && method === "POST") return response(url, 400, { name: ["taken"] });
      throw new Error("unexpected");
    }) as never);
    renderTab();
    const backlog = (await screen.findAllByTestId("state-group-card"))[0]!;
    await user.click(within(backlog).getByRole("button", { name: "Add a state to Backlog" }));
    await user.type(within(backlog).getByLabelText("State name"), "Backlog");
    await user.click(within(backlog).getByRole("button", { name: "Create state" }));
    await waitFor(() => {
      expect(within(backlog).getByTestId("state-form")).toBeInTheDocument();
    });
  });
});

describe("editing, deleting and promoting", () => {
  it("swaps a row for the prefilled form and closes it after a save", async () => {
    const user = userEvent.setup();
    setViewer(20, true);
    stubApi();
    renderTab();
    const started = (await screen.findAllByTestId("state-group-card")).find(
      (card) => card.getAttribute("data-group") === "started"
    )!;
    await user.click(within(started).getByRole("button", { name: "Edit In progress" }));
    const form = within(started).getByRole("form", { name: "Edit state In progress" });
    expect(within(form).getByLabelText("State name")).toHaveValue("In progress");
    await user.click(within(form).getByRole("button", { name: "Save changes" }));
    await waitFor(() => {
      expect(within(started).queryByTestId("state-form")).not.toBeInTheDocument();
    });
  });

  it("keeps the form open after a refused save", async () => {
    const user = userEvent.setup();
    setViewer(20, true);
    stubApi({ patchOk: false });
    renderTab();
    const started = (await screen.findAllByTestId("state-group-card")).find(
      (card) => card.getAttribute("data-group") === "started"
    )!;
    await user.click(within(started).getByRole("button", { name: "Edit In progress" }));
    await user.click(within(started).getByRole("button", { name: "Save changes" }));
    await waitFor(() => {
      expect(within(started).getByTestId("state-form")).toBeInTheDocument();
    });
  });

  it("confirms deletion and closes the dialog on success", async () => {
    const user = userEvent.setup();
    setViewer(20, true);
    stubApi();
    renderTab();
    const started = (await screen.findAllByTestId("state-group-card")).find(
      (card) => card.getAttribute("data-group") === "started"
    )!;
    await user.click(within(started).getByRole("button", { name: "Delete In progress" }));
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete state" }));
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    });
  });

  it("keeps the dialog open when the server refuses the deletion", async () => {
    const user = userEvent.setup();
    setViewer(20, true);
    stubApi({ deleteOk: false });
    renderTab();
    const started = (await screen.findAllByTestId("state-group-card")).find(
      (card) => card.getAttribute("data-group") === "started"
    )!;
    await user.click(within(started).getByRole("button", { name: "Delete In progress" }));
    await user.click(await screen.findByRole("button", { name: "Delete state" }));
    await waitFor(() => {
      expect(screen.getByRole("dialog")).toBeInTheDocument();
    });
  });

  it("promotes through the row action", async () => {
    const user = userEvent.setup();
    setViewer(20, true);
    const calls = stubApi();
    renderTab();
    const started = (await screen.findAllByTestId("state-group-card")).find(
      (card) => card.getAttribute("data-group") === "started"
    )!;
    await user.click(within(started).getByRole("button", { name: "Make In progress the default state" }));
    await waitFor(() => {
      expect(calls.some((call) => call.endsWith("/mark-default/"))).toBe(true);
    });
  });

  it("turns the member toggle into a project write", async () => {
    const user = userEvent.setup();
    setViewer(20, false);
    const calls = stubApi();
    renderTab();
    await user.click(await screen.findByRole("switch", { name: "Let members edit states" }));
    await waitFor(() => {
      expect(calls.some((call) => call.startsWith("PATCH ") && call.endsWith(`/projects/${TEST_PROJECT}/`))).toBe(true);
    });
  });

  it("moves a row through a drop", async () => {
    const user = userEvent.setup();
    setViewer(20, true);
    const calls = stubApi({ patchOk: false });
    renderTab();
    const backlog = (await screen.findAllByTestId("state-group-card"))[0]!;
    const rows = within(backlog).getAllByTestId("state-row");
    fireEvent.dragStart(within(backlog).getAllByTestId("state-row-grip")[0]!, {
      dataTransfer: { setData: vi.fn(), effectAllowed: "" },
    });
    fireEvent.drop(rows[1]!);
    await waitFor(() => {
      expect(calls.some((call) => call.startsWith("PATCH ") && call.includes(`/states/${testRowId("Backlog")}/`))).toBe(
        true
      );
    });
    await user.tab();
  });

  it("reports the in-flight promotion on its own row", async () => {
    setViewer(20, true);
    let release: (value: TransportResponse) => void = () => undefined;
    const held = new Promise<TransportResponse>((resolve) => {
      release = resolve;
    });
    createAppClient("https://api.test", (async (url: string, init?: { method?: string }) => {
      const method = init?.method ?? "GET";
      if (url.endsWith("/auth/get-csrf-token/")) return response(url, 200, { csrf_token: "csrf" });
      if (url.endsWith("/states/") && method === "GET") return response(url, 200, defaultRows());
      if (url.endsWith("/mark-default/")) return held;
      throw new Error("unexpected");
    }) as never);
    renderTab();
    const started = (await screen.findAllByTestId("state-group-card")).find(
      (card) => card.getAttribute("data-group") === "started"
    )!;
    await userEvent.setup().click(within(started).getByRole("button", { name: "Make In progress the default state" }));
    // The exclusive flag flips at once: the promoted row shows the static
    // marker while the write is still in flight.
    await waitFor(() => {
      expect(within(started).getByTestId("state-default-marker")).toBeInTheDocument();
    });
    release(response("mark-default", 204));
    await waitFor(() => {
      expect(within(started).getByTestId("state-default-marker")).toHaveTextContent("Default");
    });
  });

  it("ignores a drag hover with no source and a hover over the source itself", async () => {
    setViewer(20, true);
    const calls = stubApi();
    renderTab();
    const backlog = (await screen.findAllByTestId("state-group-card")).find(
      (card) => card.getAttribute("data-group") === "backlog"
    )!;
    const rows = within(backlog).getAllByTestId("state-row");
    // A hover with nothing being dragged paints no marker.
    fireEvent.dragOver(rows[1]!);
    expect(within(backlog).queryByTestId("state-drop-marker")).not.toBeInTheDocument();
    fireEvent.dragStart(within(backlog).getAllByTestId("state-row-grip")[0]!, {
      dataTransfer: { setData: vi.fn(), effectAllowed: "" },
    });
    // Leaving a row when nothing is marked leaves nothing to clear.
    fireEvent.dragLeave(rows[1]!);
    expect(within(backlog).queryByTestId("state-drop-marker")).not.toBeInTheDocument();
    // Hovering one row and then leaving it clears that marker.
    fireEvent.dragOver(rows[1]!);
    expect(within(backlog).getByTestId("state-drop-marker")).toBeInTheDocument();
    fireEvent.dragLeave(rows[1]!);
    expect(within(backlog).queryByTestId("state-drop-marker")).not.toBeInTheDocument();
    // Hovering the source row itself never marks it as its own target.
    fireEvent.dragOver(rows[0]!);
    expect(within(backlog).queryByTestId("state-drop-marker")).not.toBeInTheDocument();
    fireEvent.drop(rows[0]!);
    expect(calls.some((call) => call.startsWith("PATCH "))).toBe(false);
  });

  it("keeps the marker when the pointer leaves a row other than the marked one", async () => {
    setViewer(20, true);
    stubApi();
    renderTab();
    const backlog = (await screen.findAllByTestId("state-group-card")).find(
      (card) => card.getAttribute("data-group") === "backlog"
    )!;
    const rows = within(backlog).getAllByTestId("state-row");
    fireEvent.dragStart(within(backlog).getAllByTestId("state-row-grip")[0]!, {
      dataTransfer: { setData: vi.fn(), effectAllowed: "" },
    });
    fireEvent.dragOver(rows[1]!);
    expect(within(backlog).getByTestId("state-drop-marker")).toBeInTheDocument();
    fireEvent.dragLeave(rows[0]!);
    expect(within(backlog).getByTestId("state-drop-marker")).toBeInTheDocument();
  });

  it("moves a state across cards when the source was dragged in another group", async () => {
    setViewer(20, true);
    const calls = stubApi();
    renderTab(defaultRows());
    const cards = await screen.findAllByTestId("state-group-card");
    const backlog = cards.find((card) => card.getAttribute("data-group") === "backlog")!;
    const unstarted = cards.find((card) => card.getAttribute("data-group") === "unstarted")!;
    const target = within(unstarted).getByTestId("state-row");
    fireEvent.dragStart(within(backlog).getAllByTestId("state-row-grip")[1]!, {
      dataTransfer: { setData: vi.fn(), effectAllowed: "" },
    });
    fireEvent.dragOver(target);
    expect(within(unstarted).getByTestId("state-drop-marker")).toBeInTheDocument();
    fireEvent.drop(target);
    await waitFor(() => {
      expect(
        calls.some(
          (call) =>
            call.startsWith("PATCH ") &&
            call.includes(`/states/${testRowId("Triage queue")}/`) &&
            call.endsWith(`/projects/${TEST_PROJECT}/states/${testRowId("Triage queue")}/`)
        )
      ).toBe(true);
    });
    expect(calls.filter((call) => call.startsWith("PATCH "))).toHaveLength(1);
  });

  it("lands a cross-card drop in a group that holds a single state", async () => {
    setViewer(20, true);
    const calls = stubApi();
    renderTab();
    const cards = await screen.findAllByTestId("state-group-card");
    const backlog = cards.find((card) => card.getAttribute("data-group") === "backlog")!;
    const review = cards.find((card) => card.getAttribute("data-group") === "review")!;
    fireEvent.click(within(review).getByTestId("state-group-toggle"));
    const target = within(review).getByTestId("state-row");
    fireEvent.dragStart(within(backlog).getAllByTestId("state-row-grip")[0]!, {
      dataTransfer: { setData: vi.fn(), effectAllowed: "" },
    });
    fireEvent.drop(target);
    await waitFor(() => {
      expect(calls.some((call) => call.startsWith("PATCH ") && call.includes(`/states/${testRowId("Backlog")}/`))).toBe(
        true
      );
    });
  });

  it("does not send a drop that leaves the row where it was", async () => {
    setViewer(20, true);
    const calls = stubApi();
    renderTab();
    const started = (await screen.findAllByTestId("state-group-card")).find(
      (card) => card.getAttribute("data-group") === "started"
    )!;
    const rows = within(started).getAllByTestId("state-row");
    // Beta already sits directly below Alpha: dropping it there again
    // resolves to the same slot and sends no write. jsdom drag events drop
    // coordinates, so the hovered edge resolves to the lower half.
    fireEvent.dragStart(within(started).getAllByTestId("state-row-grip")[1]!, {
      dataTransfer: { setData: vi.fn(), effectAllowed: "" },
    });
    fireEvent.drop(rows[0]!);
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(rows).toHaveLength(2);
    expect(calls.some((call) => call.startsWith("PATCH "))).toBe(false);
  });
});
