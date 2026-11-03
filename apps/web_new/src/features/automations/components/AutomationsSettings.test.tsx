// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Automations page behavior (parity/specs/agents.md §8, AGT-034–037).
// A stub transport stands in for Django: the project-details collection,
// the state list, the project roles and the caller's membership. Every
// PATCH records its body so the tests can assert exactly what the rows
// wrote, and failed updates leave the stored row untouched.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ProjectDetails, State } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifyError: vi.fn() };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { notifyError } from "../../../shared/shell/index.js";
import { AUTOMATION_UPDATE_FAILED_MESSAGE, AUTOMATION_UPDATE_FAILED_TITLE } from "../api/automationQueries.js";
import {
  ARCHIVE_DELAY_LEAD,
  ARCHIVE_ROW_DESCRIPTION,
  ARCHIVE_ROW_TITLE,
  AUTOMATIONS_INTRO,
  AUTOMATIONS_ROWS_SLOT,
  AUTOMATIONS_UNAVAILABLE_MESSAGE,
  CLOSE_DELAY_LEAD,
  CLOSE_ROW_DESCRIPTION,
  CLOSE_ROW_TITLE,
  STATE_FALLBACK_LABEL,
} from "../index.js";
import { AutomationsScreen, AutomationsSettings } from "./AutomationsSettings.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const WORKSPACE = "323e4567-e89b-12d3-a456-426614174002";
const OTHER_PROJECT = "223e4567-e89b-12d3-a456-426614174009";
const DROPPED = "123e4567-e89b-12d3-a456-42661417400a";
const WONT_DO = "123e4567-e89b-12d3-a456-42661417400b";

function projectRow(overrides: Partial<ProjectDetails> = {}): ProjectDetails {
  return {
    id: PROJECT,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: WORKSPACE,
    name: "Website relaunch",
    description: "",
    description_text: null,
    description_html: null,
    network: 0,
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
    members_can_edit_states: false,
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
    sort_order: 10000,
    member_role: 20,
    anchor: null,
    members: [],
    next_work_item_sequence: 1,
    agent_executor_options: [],
    default_agent_executor: "local_runner",
    ...overrides,
  };
}

function stateRow(id: string, name: string, group: State["group"]): State {
  return {
    id,
    project_id: PROJECT,
    workspace_id: WORKSPACE,
    name,
    color: "#6b7280",
    group,
    default: false,
    description: "",
    sequence: 0,
  };
}

interface StubOptions {
  project?: Record<string, unknown>;
  states?: State[];
  status?: number;
  roles?: Record<string, number>;
  memberRole?: number | null;
  patchStatus?: number;
  hangProjects?: boolean;
  hangStates?: boolean;
  statesStatus?: number;
  rolesStatus?: number;
  membershipStatus?: number;
  membershipBody?: Record<string, unknown>;
}

interface Call {
  method: string;
  url: string;
  body: string;
}

function stubTransport(options: StubOptions = {}) {
  const calls: Call[] = [];
  let row = projectRow(options.project);
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string } = {}) => {
    const method = init.method ?? "GET";
    calls.push({ method, url, body: init.body ?? "" });
    const json = (status: number, body: unknown) => ({
      status,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(body),
    });
    if (url.endsWith("/auth/get-csrf-token/")) return json(200, { csrf_token: "csrf" });
    if (url.includes("/project-roles/")) {
      if (options.rolesStatus !== undefined) return json(options.rolesStatus, {});
      return json(200, options.roles ?? { [PROJECT]: 20 });
    }
    if (url.includes("/workspace-members/me/")) {
      if (options.membershipStatus !== undefined) return json(options.membershipStatus, {});
      if (options.membershipBody !== undefined) return json(200, options.membershipBody);
      return json(200, {
        id: "423e4567-e89b-12d3-a456-426614174003",
        workspace: WORKSPACE,
        member: "523e4567-e89b-12d3-a456-426614174004",
        role: options.memberRole === undefined ? 20 : options.memberRole,
        company_role: null,
        view_props: {},
        default_props: {},
        issue_props: {},
        is_active: true,
        getting_started_checklist: {},
        tips: {},
        explored_features: {},
        draft_issue_count: 0,
        created_by: null,
        updated_by: null,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      });
    }
    if (url.endsWith(`/api/workspaces/${SLUG}/projects/details/`)) {
      if (options.hangProjects) return new Promise<never>(() => undefined);
      if (options.status !== undefined) return json(options.status, {});
      return json(200, [row, projectRow({ id: OTHER_PROJECT, name: "Other", identifier: "OTH" })]);
    }
    if (url.endsWith(`/api/workspaces/${SLUG}/projects/${PROJECT}/states/`)) {
      if (options.hangStates) return new Promise<never>(() => undefined);
      if (options.statesStatus !== undefined) return json(options.statesStatus, {});
      return json(200, options.states ?? []);
    }
    if (url.endsWith(`/api/workspaces/${SLUG}/projects/${PROJECT}/`) && method === "PATCH") {
      if (options.patchStatus !== undefined && options.patchStatus >= 400) return json(options.patchStatus, {});
      row = { ...row, ...(JSON.parse(init.body ?? "{}") as Partial<ProjectDetails>) };
      return json(200, row);
    }
    throw new Error(`unexpected request ${method} ${url}`);
  }) as never);
  return calls;
}

function setup(options: StubOptions = {}) {
  const calls = stubTransport(options);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <AutomationsSettings workspaceSlug={SLUG} projectId={PROJECT} />
    </QueryClientProvider>
  );
  return { calls, queryClient, ...view };
}

function row(title: string): HTMLElement {
  const heading = screen.getByRole("heading", { level: 4, name: title });
  const element = heading.closest("[data-automation-row]");
  if (!(element instanceof HTMLElement)) throw new Error(`no row for ${title}`);
  return element;
}

function rowSwitch(title: string): HTMLElement {
  return within(row(title)).getByRole("switch", { name: title });
}

function patches(calls: Call[]): string[] {
  return calls.filter((call) => call.method === "PATCH").map((call) => call.body);
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
  vi.restoreAllMocks();
});

describe("AutomationsSettings gate", () => {
  it("shows the loading state until the standing reads settle", () => {
    setup({ hangProjects: true });
    expect(screen.getByRole("status")).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: ARCHIVE_ROW_TITLE })).not.toBeInTheDocument();
  });

  it("refuses a project member with the not-authorized view", async () => {
    setup({ roles: { [PROJECT]: 15 }, memberRole: 15 });
    expect(
      await screen.findByRole("heading", { name: "Oops! You are not authorized to view this page" })
    ).toBeInTheDocument();
    expect(screen.queryByTestId("automations-settings")).not.toBeInTheDocument();
  });

  it("refuses a guest with the not-authorized view", async () => {
    setup({ roles: { [PROJECT]: 5 }, memberRole: 5 });
    expect(
      await screen.findByRole("heading", { name: "Oops! You are not authorized to view this page" })
    ).toBeInTheDocument();
  });

  it("admits a workspace admin whose project role is only guest", async () => {
    setup({ roles: { [PROJECT]: 5 }, memberRole: 20, states: [stateRow(DROPPED, "Dropped", "cancelled")] });
    expect(await screen.findByRole("heading", { level: 4, name: ARCHIVE_ROW_TITLE })).toBeInTheDocument();
    expect(screen.queryByTestId("automations-not-authorized")).not.toBeInTheDocument();
  });

  it("does not lift a plain member when the role map omits the project", async () => {
    setup({ roles: {}, memberRole: 15, states: [stateRow(DROPPED, "Dropped", "cancelled")] });
    expect(
      await screen.findByRole("heading", { name: "Oops! You are not authorized to view this page" })
    ).toBeInTheDocument();
  });

  it("keeps a project admin when no workspace membership row resolves", async () => {
    setup({ roles: { [PROJECT]: 20 }, membershipBody: {}, states: [stateRow(DROPPED, "Dropped", "cancelled")] });
    expect(await screen.findByRole("heading", { level: 4, name: ARCHIVE_ROW_TITLE })).toBeInTheDocument();
    expect(screen.queryByTestId("automations-not-authorized")).not.toBeInTheDocument();
  });

  it("refuses a project member when no workspace membership row resolves", async () => {
    setup({ roles: { [PROJECT]: 15 }, membershipBody: {}, states: [stateRow(DROPPED, "Dropped", "cancelled")] });
    expect(
      await screen.findByRole("heading", { name: "Oops! You are not authorized to view this page" })
    ).toBeInTheDocument();
  });

  it("refuses a member whose membership row carries no role", async () => {
    setup({ roles: { [PROJECT]: 15 }, memberRole: null });
    expect(
      await screen.findByRole("heading", { name: "Oops! You are not authorized to view this page" })
    ).toBeInTheDocument();
  });

  it("refuses a known member when the membership read fails", async () => {
    setup({ roles: { [PROJECT]: 15 }, membershipStatus: 500 });
    expect(
      await screen.findByRole("heading", { name: "Oops! You are not authorized to view this page" })
    ).toBeInTheDocument();
    expect(screen.queryByTestId("automations-settings")).not.toBeInTheDocument();
  });

  it("refuses a known guest when the membership read fails", async () => {
    setup({ roles: { [PROJECT]: 5 }, membershipStatus: 500 });
    expect(
      await screen.findByRole("heading", { name: "Oops! You are not authorized to view this page" })
    ).toBeInTheDocument();
    expect(screen.queryByTestId("automations-settings")).not.toBeInTheDocument();
  });

  it("refuses without admin evidence when the role read fails", async () => {
    setup({ rolesStatus: 500 });
    expect(
      await screen.findByRole("heading", { name: "Oops! You are not authorized to view this page" })
    ).toBeInTheDocument();
    expect(screen.queryByTestId("automations-settings")).not.toBeInTheDocument();
  });

  it("refuses when both standing reads fail", async () => {
    setup({ rolesStatus: 500, membershipStatus: 500 });
    expect(
      await screen.findByRole("heading", { name: "Oops! You are not authorized to view this page" })
    ).toBeInTheDocument();
    expect(screen.queryByTestId("automations-settings")).not.toBeInTheDocument();
  });

  it("renders the built-in rows and an empty extension slot for an admin", async () => {
    setup({ states: [stateRow(DROPPED, "Dropped", "cancelled")] });
    expect(await screen.findByRole("heading", { level: 4, name: ARCHIVE_ROW_TITLE })).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 4, name: CLOSE_ROW_TITLE })).toBeInTheDocument();
    expect(screen.getByText("Automations")).toBeInTheDocument();
    const slot = screen.getByTestId("automations-extension");
    expect(slot).toBeEmptyDOMElement();
    expect(within(row(ARCHIVE_ROW_TITLE)).getByText(ARCHIVE_ROW_DESCRIPTION)).toBeInTheDocument();
    expect(within(row(CLOSE_ROW_TITLE)).getByText(CLOSE_ROW_DESCRIPTION)).toBeInTheDocument();
    expect(screen.getByText(AUTOMATIONS_INTRO)).toBeInTheDocument();
    expect(within(row(ARCHIVE_ROW_TITLE)).queryByRole("button", { name: /months?/ })).toBeNull();
  });

  it("shows an unavailable notice instead of enabled rows when the project read fails", async () => {
    setup({ status: 500, states: [stateRow(DROPPED, "Dropped", "cancelled")] });
    expect(await screen.findByRole("alert")).toHaveTextContent(AUTOMATIONS_UNAVAILABLE_MESSAGE);
    expect(screen.queryByRole("switch", { name: ARCHIVE_ROW_TITLE })).toBeNull();
    expect(screen.queryByRole("switch", { name: CLOSE_ROW_TITLE })).toBeNull();
  });

  it("shows the same notice instead of enabled rows when the state read fails", async () => {
    const calls = stubTransport({ statesStatus: 500 });
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <AutomationsSettings workspaceSlug={SLUG} projectId={PROJECT} />
      </QueryClientProvider>
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(AUTOMATIONS_UNAVAILABLE_MESSAGE);
    expect(screen.queryByRole("switch", { name: CLOSE_ROW_TITLE })).toBeNull();
    expect(patches(calls)).toEqual([]);
  });
});

describe("AutomationsScreen", () => {
  it("renders a caller-supplied extension below the built-in rows", async () => {
    render(
      <AutomationsScreen
        project={projectRow({ archive_in: 1 })}
        projectPending={false}
        states={[]}
        onChange={vi.fn()}
        extensions={<p>Edition automation</p>}
      />
    );
    const slot = screen.getByTestId("automations-extension");
    expect(within(slot).getByText("Edition automation")).toBeInTheDocument();
    const headings = within(screen.getByRole("region", { name: "Automations" })).getAllByRole("heading", { level: 4 });
    expect(headings.map((heading) => heading.textContent)).toEqual([ARCHIVE_ROW_TITLE, CLOSE_ROW_TITLE]);
    // The built-in rows keep their own container, so a reader scoped to it
    // never picks up an edition's rows from the sibling slot (AGT-037).
    const builtIn = within(screen.getByTestId(AUTOMATIONS_ROWS_SLOT));
    expect(builtIn.getAllByRole("heading", { level: 4 }).map((heading) => heading.textContent)).toEqual([
      ARCHIVE_ROW_TITLE,
      CLOSE_ROW_TITLE,
    ]);
    expect(builtIn.queryByText("Edition automation")).toBeNull();
  });

  it("shows the row skeletons while the project collection loads", () => {
    render(<AutomationsScreen project={undefined} projectPending states={[]} onChange={vi.fn()} extensions={null} />);
    expect(screen.getByRole("status", { name: "Loading automations" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { level: 4 })).toBeNull();
    expect(screen.getByTestId("automations-extension")).toBeEmptyDOMElement();
  });
});

describe("archive row", () => {
  it("toggles on with a one-month default and reveals the picker", async () => {
    const user = userEvent.setup();
    const { calls } = setup();
    await screen.findByRole("heading", { level: 4, name: ARCHIVE_ROW_TITLE });
    await user.click(rowSwitch(ARCHIVE_ROW_TITLE));
    await waitFor(() => expect(patches(calls)).toEqual([JSON.stringify({ archive_in: 1 })]));
    await waitFor(() => expect(rowSwitch(ARCHIVE_ROW_TITLE)).toHaveAttribute("aria-checked", "true"));
    const trigger = within(row(ARCHIVE_ROW_TITLE)).getByRole("button", {
      name: new RegExp(`^${ARCHIVE_DELAY_LEAD},`),
    });
    expect(trigger).toHaveTextContent("1 month");
  });

  it("toggles off back to zero and hides the picker", async () => {
    const user = userEvent.setup();
    const { calls } = setup({ project: { archive_in: 3 } });
    await waitFor(() => expect(rowSwitch(ARCHIVE_ROW_TITLE)).toHaveAttribute("aria-checked", "true"));
    await user.click(rowSwitch(ARCHIVE_ROW_TITLE));
    await waitFor(() => expect(patches(calls)).toEqual([JSON.stringify({ archive_in: 0 })]));
    await waitFor(() => expect(rowSwitch(ARCHIVE_ROW_TITLE)).toHaveAttribute("aria-checked", "false"));
    expect(
      within(row(ARCHIVE_ROW_TITLE)).queryByRole("button", { name: new RegExp(`^${ARCHIVE_DELAY_LEAD},`) })
    ).toBeNull();
  });

  it("persists a preset from the open picker", async () => {
    const user = userEvent.setup();
    const { calls } = setup({ project: { archive_in: 1 } });
    await waitFor(() => expect(rowSwitch(ARCHIVE_ROW_TITLE)).toHaveAttribute("aria-checked", "true"));
    await user.click(
      within(row(ARCHIVE_ROW_TITLE)).getByRole("button", { name: new RegExp(`^${ARCHIVE_DELAY_LEAD},`) })
    );
    await user.click(screen.getByRole("option", { name: "6 months" }));
    await waitFor(() => expect(patches(calls)).toEqual([JSON.stringify({ archive_in: 6 })]));
    await waitFor(() =>
      expect(
        within(row(ARCHIVE_ROW_TITLE)).getByRole("button", { name: new RegExp(`^${ARCHIVE_DELAY_LEAD},`) })
      ).toHaveTextContent("6 months")
    );
  });

  it("forwards a custom month from the dialog", async () => {
    const user = userEvent.setup();
    const { calls } = setup({ project: { archive_in: 1 } });
    await waitFor(() => expect(rowSwitch(ARCHIVE_ROW_TITLE)).toHaveAttribute("aria-checked", "true"));
    await user.click(
      within(row(ARCHIVE_ROW_TITLE)).getByRole("button", { name: new RegExp(`^${ARCHIVE_DELAY_LEAD},`) })
    );
    await user.click(screen.getByRole("button", { name: "Customize time range" }));
    const dialog = await screen.findByRole("dialog", { name: "Customize time range" });
    const field = within(dialog).getByPlaceholderText("Enter Months");
    await user.clear(field);
    await user.type(field, "7");
    await user.click(within(dialog).getByRole("button", { name: "Submit" }));
    await waitFor(() => expect(patches(calls)).toEqual([JSON.stringify({ archive_in: 7 })]));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });
});

describe("close row", () => {
  it("holds the rows until the state list answers, so enabling never writes a null target", async () => {
    const user = userEvent.setup();
    const { calls } = setup({ hangStates: true });
    const loading = await screen.findByRole("status", { name: "Loading automations" });
    expect(loading).toBeInTheDocument();
    expect(screen.queryByRole("switch", { name: CLOSE_ROW_TITLE })).toBeNull();
    expect(screen.queryByRole("switch", { name: ARCHIVE_ROW_TITLE })).toBeNull();
    expect(patches(calls)).toEqual([]);
    await user.click(document.body);
    expect(patches(calls)).toEqual([]);
  });

  it("toggles on with a one-month delay and the first cancelled state", async () => {
    const user = userEvent.setup();
    const { calls } = setup({
      states: [
        stateRow("123e4567-e89b-12d3-a456-42661417400c", "Todo", "unstarted"),
        stateRow(DROPPED, "Dropped", "cancelled"),
      ],
    });
    await screen.findByRole("heading", { level: 4, name: CLOSE_ROW_TITLE });
    await user.click(rowSwitch(CLOSE_ROW_TITLE));
    await waitFor(() => expect(patches(calls)).toEqual([JSON.stringify({ close_in: 1, default_state: DROPPED })]));
    await waitFor(() => expect(rowSwitch(CLOSE_ROW_TITLE)).toHaveAttribute("aria-checked", "true"));
    expect(within(row(CLOSE_ROW_TITLE)).getByText("Dropped")).toBeInTheDocument();
    const stateButton = within(row(CLOSE_ROW_TITLE)).getByText("Dropped").closest("button");
    expect(stateButton).toBeDisabled();
  });

  it("shows the State placeholder and a closed picker when nothing is cancelled", async () => {
    const user = userEvent.setup();
    setup({ states: [stateRow("123e4567-e89b-12d3-a456-42661417400c", "Todo", "unstarted")] });
    await screen.findByRole("heading", { level: 4, name: CLOSE_ROW_TITLE });
    await user.click(rowSwitch(CLOSE_ROW_TITLE));
    const button = await within(row(CLOSE_ROW_TITLE)).findByRole("button", { name: STATE_FALLBACK_LABEL });
    expect(button).toBeDisabled();
  });

  it("offers every cancelled state once a second exists and persists the pick", async () => {
    const user = userEvent.setup();
    const { calls } = setup({
      project: { close_in: 2, default_state: DROPPED },
      states: [stateRow(DROPPED, "Dropped", "cancelled"), stateRow(WONT_DO, "Won't do", "cancelled")],
    });
    await waitFor(() => expect(rowSwitch(CLOSE_ROW_TITLE)).toHaveAttribute("aria-checked", "true"));
    const trigger = within(row(CLOSE_ROW_TITLE)).getByRole("button", { name: "Dropped" });
    expect(trigger).not.toBeDisabled();
    await user.click(trigger);
    expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual(["Dropped", "Won't do"]);
    await user.click(screen.getByRole("option", { name: "Won't do" }));
    await waitFor(() => expect(patches(calls)).toEqual([JSON.stringify({ default_state: WONT_DO })]));
    await waitFor(() =>
      expect(within(row(CLOSE_ROW_TITLE)).getByRole("button", { name: "Won't do" })).toBeInTheDocument()
    );
  });

  it("clears the delay and target when switched off", async () => {
    const user = userEvent.setup();
    const { calls } = setup({
      project: { close_in: 2, default_state: DROPPED },
      states: [stateRow(DROPPED, "Dropped", "cancelled")],
    });
    await waitFor(() => expect(rowSwitch(CLOSE_ROW_TITLE)).toHaveAttribute("aria-checked", "true"));
    await user.click(rowSwitch(CLOSE_ROW_TITLE));
    await waitFor(() => expect(patches(calls)).toEqual([JSON.stringify({ close_in: 0, default_state: null })]));
    await waitFor(() => expect(rowSwitch(CLOSE_ROW_TITLE)).toHaveAttribute("aria-checked", "false"));
  });

  it("persists a preset delay", async () => {
    const user = userEvent.setup();
    const { calls } = setup({
      project: { close_in: 1, default_state: DROPPED },
      states: [stateRow(DROPPED, "Dropped", "cancelled")],
    });
    await waitFor(() => expect(rowSwitch(CLOSE_ROW_TITLE)).toHaveAttribute("aria-checked", "true"));
    await user.click(within(row(CLOSE_ROW_TITLE)).getByRole("button", { name: new RegExp(`^${CLOSE_DELAY_LEAD},`) }));
    await user.click(screen.getByRole("option", { name: "3 months" }));
    await waitFor(() => expect(patches(calls)).toEqual([JSON.stringify({ close_in: 3 })]));
  });
});

describe("failure handling", () => {
  it("toasts the generic notice and keeps the stored value on a refused archive update", async () => {
    const user = userEvent.setup();
    setup({ patchStatus: 500 });
    await screen.findByRole("heading", { level: 4, name: ARCHIVE_ROW_TITLE });
    await user.click(rowSwitch(ARCHIVE_ROW_TITLE));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith(AUTOMATION_UPDATE_FAILED_TITLE, AUTOMATION_UPDATE_FAILED_MESSAGE)
    );
    expect(rowSwitch(ARCHIVE_ROW_TITLE)).toHaveAttribute("aria-checked", "false");
  });

  it("routes a refused close update through the same notice", async () => {
    const user = userEvent.setup();
    setup({ patchStatus: 500, states: [stateRow(DROPPED, "Dropped", "cancelled")] });
    await screen.findByRole("heading", { level: 4, name: CLOSE_ROW_TITLE });
    await user.click(rowSwitch(CLOSE_ROW_TITLE));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith(AUTOMATION_UPDATE_FAILED_TITLE, AUTOMATION_UPDATE_FAILED_MESSAGE)
    );
    expect(rowSwitch(CLOSE_ROW_TITLE)).toHaveAttribute("aria-checked", "false");
  });
});
