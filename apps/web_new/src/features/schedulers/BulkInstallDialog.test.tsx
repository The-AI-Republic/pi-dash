// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ApiError, type ProjectDetails, type Scheduler } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";

const { contracts } = vi.hoisted(() => ({
  contracts: {
    createSchedulerBinding: vi.fn(),
    listProjectDetails: vi.fn(),
    listSchedulerBindings: vi.fn(),
  },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});
vi.mock("../../shared/shell/index.js", () => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));

import { notifyError, notifySuccess } from "../../shared/shell/index.js";
import { BulkInstallDialog, joinedProjects, projectName } from "./BulkInstallDialog.js";

const SLUG = "acme";
const WS_ID = "123e4567-e89b-12d3-a456-426614174000";
const ALPHA = "223e4567-e89b-12d3-a456-426614174001";
const BETA = "223e4567-e89b-12d3-a456-426614174002";
const GAMMA = "223e4567-e89b-12d3-a456-426614174003";

const SCHEDULER: Scheduler = {
  id: "s1",
  workspace: "w1",
  slug: "weekly-review",
  name: "Weekly review",
  description: "Look at the week.",
  prompt: "Review the week",
  color: "#5e6ad2",
  source: "builtin",
  is_enabled: true,
  active_binding_count: 0,
  created_at: "2026-10-01T00:00:00Z",
  updated_at: "2026-10-01T00:00:00Z",
};

function project(overrides: Partial<ProjectDetails> = {}): ProjectDetails {
  return {
    id: ALPHA,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: WS_ID,
    name: "Alpha",
    description: "",
    description_text: null,
    description_html: null,
    network: 2,
    identifier: "ALP",
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
    sort_order: null,
    member_role: 15,
    anchor: null,
    members: [],
    next_work_item_sequence: 1,
    agent_executor_options: [
      { kind: "cloud_agent", available: false, reason_code: "cloud_agent_unavailable" },
      { kind: "local_runner", available: true, reason_code: "" },
      { kind: "managed_runner", available: false, reason_code: "managed_runner_disabled" },
    ],
    default_agent_executor: "local_runner",
    ...overrides,
  };
}

function binding(projectId: string, schedulerId = SCHEDULER.id) {
  return {
    id: `b-${projectId}`,
    scheduler: schedulerId,
    scheduler_slug: "weekly-review",
    scheduler_name: "Weekly review",
    scheduler_color: "#5e6ad2",
    project: projectId,
    workspace: "w1",
    dtstart: "2026-10-20T09:00:00Z",
    tzid: "UTC",
    rrule: "FREQ=DAILY",
    rdates: [],
    exdates: [],
    extra_context: "",
    enabled: true,
    outcome_mode: "create_issue",
    pod: null,
    pod_name: null,
    next_run_at: null,
    last_run: null,
    last_run_status: null,
    last_run_ended_at: null,
    last_error: "",
    actor: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
  };
}

function renderDialog(scheduler: Scheduler | null = SCHEDULER) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={queryClient}>
      <BulkInstallDialog workspaceSlug={SLUG} scheduler={scheduler} onClose={onClose} />
    </QueryClientProvider>
  );
  return { onClose, user: userEvent.setup() };
}

async function openPicker(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: /project/ }));
  return screen.findByRole("listbox", { name: "Projects" });
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
  contracts.listSchedulerBindings.mockResolvedValue([]);
});
afterEach(() => {
  cleanup();
  resetAppClient();
});

describe("joinedProjects", () => {
  it("keeps joined, unarchived projects only", () => {
    const joined = project({ id: ALPHA });
    const outsider = project({ id: BETA, member_role: null });
    const archived = project({ id: GAMMA, archived_at: "2024-05-06T07:08:09Z" });
    expect(joinedProjects([joined, outsider, archived])).toEqual([joined]);
  });
});

describe("BulkInstallDialog", () => {
  it("stays unmounted while no definition is chosen", () => {
    contracts.listProjectDetails.mockResolvedValue([project()]);
    renderDialog(null);
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
    expect(contracts.listProjectDetails).not.toHaveBeenCalled();
  });

  it("offers only joined projects and locks those already installed", async () => {
    contracts.listProjectDetails.mockResolvedValue([
      project({ id: ALPHA, name: "Alpha" }),
      project({ id: BETA, name: "Beta" }),
      project({ id: GAMMA, name: "Gamma", member_role: null }),
      project({ id: "423e4567-e89b-12d3-a456-426614174004", name: "Archived", archived_at: "2024-05-06T07:08:09Z" }),
    ]);
    contracts.listSchedulerBindings.mockImplementation((_client: unknown, _slug: string, projectId: string) =>
      Promise.resolve(projectId === BETA ? [binding(BETA)] : [])
    );
    const { user } = renderDialog();
    const listbox = await openPicker(user);

    await waitFor(() => expect(within(listbox).getByLabelText(/Beta/)).toBeDisabled());
    expect(within(listbox).getByLabelText(/Alpha/)).not.toBeDisabled();
    expect(within(listbox).queryByText("Gamma")).not.toBeInTheDocument();
    expect(within(listbox).queryByText("Archived")).not.toBeInTheDocument();
    expect(within(listbox).getByText("Installed")).toBeInTheDocument();
    expect(within(listbox).getByLabelText(/Beta/)).toBeChecked();
  });

  it("leaves a target offerable when its installs cannot be read", async () => {
    contracts.listProjectDetails.mockResolvedValue([project({ id: ALPHA, name: "Alpha" })]);
    contracts.listSchedulerBindings.mockRejectedValue(
      new ApiError({ status: 500, code: "server_error", message: "Nope", fields: {} })
    );
    const { user } = renderDialog();
    const listbox = await openPicker(user);
    await waitFor(() => expect(within(listbox).getByLabelText(/Alpha/)).not.toBeDisabled());
    expect(within(listbox).queryByText("Installed")).not.toBeInTheDocument();
  });

  it("searches projects by name and identifier", async () => {
    contracts.listProjectDetails.mockResolvedValue([
      project({ id: ALPHA, name: "Alpha", identifier: "ALP" }),
      project({ id: BETA, name: "Beta", identifier: "BET" }),
    ]);
    const { user } = renderDialog();
    const listbox = await openPicker(user);
    await within(listbox).findByLabelText(/Alpha/);

    await user.type(screen.getByLabelText("Search projects"), "bet");
    expect(within(listbox).queryByLabelText(/Alpha/)).not.toBeInTheDocument();
    expect(within(listbox).getByLabelText(/Beta/)).toBeInTheDocument();

    await user.clear(screen.getByLabelText("Search projects"));
    await user.type(screen.getByLabelText("Search projects"), "nothing");
    expect(within(listbox).getByText("No projects match your search.")).toBeInTheDocument();
  });

  it("selects every unlocked target, then clears them", async () => {
    contracts.listProjectDetails.mockResolvedValue([
      project({ id: ALPHA, name: "Alpha" }),
      project({ id: BETA, name: "Beta" }),
      project({ id: GAMMA, name: "Gamma" }),
    ]);
    contracts.listSchedulerBindings.mockImplementation((_client: unknown, _slug: string, projectId: string) =>
      Promise.resolve(projectId === GAMMA ? [binding(GAMMA)] : [])
    );
    const { user } = renderDialog();
    const listbox = await openPicker(user);
    await waitFor(() => expect(within(listbox).getByLabelText(/Gamma/)).toBeDisabled());

    await user.click(screen.getByRole("button", { name: "Select all" }));
    expect(within(listbox).getByLabelText(/Alpha/)).toBeChecked();
    expect(within(listbox).getByLabelText(/Beta/)).toBeChecked();
    expect(within(listbox).getByLabelText(/Gamma/)).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "Clear selection" }));
    expect(within(listbox).getByLabelText(/Alpha/)).not.toBeChecked();
    expect(within(listbox).getByLabelText(/Beta/)).not.toBeChecked();
  });

  it("counts only picked targets in the summary, never the locked ones", async () => {
    contracts.listProjectDetails.mockResolvedValue([
      project({ id: ALPHA, name: "Alpha" }),
      project({ id: BETA, name: "Beta" }),
    ]);
    contracts.listSchedulerBindings.mockImplementation((_client: unknown, _slug: string, projectId: string) =>
      Promise.resolve(projectId === BETA ? [binding(BETA)] : [])
    );
    const { user } = renderDialog();
    const listbox = await openPicker(user);
    await waitFor(() => expect(within(listbox).getByLabelText(/Beta/)).toBeDisabled());
    const trigger = screen.getByRole("button", { name: /project/ });

    expect(trigger).toHaveTextContent("Select projects");
    await user.click(within(listbox).getByLabelText(/Alpha/));
    expect(trigger).toHaveTextContent("1 project selected");
    await user.click(within(listbox).getByLabelText(/Alpha/));
    expect(trigger).toHaveTextContent("Select projects");
  });

  it("scopes select-all and its label to the search-filtered targets", async () => {
    contracts.listProjectDetails.mockResolvedValue([
      project({ id: ALPHA, name: "Alpha" }),
      project({ id: BETA, name: "Beta" }),
    ]);
    const { user } = renderDialog();
    const listbox = await openPicker(user);
    await within(listbox).findByLabelText(/Alpha/);

    await user.type(screen.getByLabelText("Search projects"), "alpha");
    expect(screen.getByRole("button", { name: "Select all" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Select all" }));
    expect(screen.getByRole("button", { name: "Clear selection" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /project/ })).toHaveTextContent("1 project selected");

    await user.clear(screen.getByLabelText("Search projects"));
    expect(within(listbox).getByLabelText(/Alpha/)).toBeChecked();
    expect(within(listbox).getByLabelText(/Beta/)).not.toBeChecked();

    await user.click(screen.getByRole("button", { name: "Select all" }));
    expect(within(listbox).getByLabelText(/Beta/)).toBeChecked();
    expect(screen.getByRole("button", { name: /project/ })).toHaveTextContent("2 projects selected");

    await user.type(screen.getByLabelText("Search projects"), "alpha");
    await user.click(screen.getByRole("button", { name: "Clear selection" }));
    expect(screen.getByRole("button", { name: /project/ })).toHaveTextContent("1 project selected");
    await user.clear(screen.getByLabelText("Search projects"));
    expect(within(listbox).getByLabelText(/Alpha/)).not.toBeChecked();
    expect(within(listbox).getByLabelText(/Beta/)).toBeChecked();
  });

  it("refuses to submit without a target", async () => {
    contracts.listProjectDetails.mockResolvedValue([project({ id: ALPHA, name: "Alpha" })]);
    const { user, onClose } = renderDialog();
    await openPicker(user);
    await user.click(screen.getByRole("button", { name: "Install" }));
    expect(notifyError).toHaveBeenCalledWith(
      "Select a project",
      "Pick at least one project to install this scheduler on."
    );
    expect(contracts.createSchedulerBinding).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("ignores a second submit while the first is in flight", async () => {
    contracts.listProjectDetails.mockResolvedValue([project({ id: ALPHA, name: "Alpha" })]);
    contracts.createSchedulerBinding.mockReturnValue(new Promise(() => undefined));
    const { user } = renderDialog();
    const listbox = await openPicker(user);
    await waitFor(() => expect(within(listbox).getByLabelText(/Alpha/)).not.toBeDisabled());
    await user.click(within(listbox).getByLabelText(/Alpha/));
    await user.click(screen.getByRole("button", { name: "Install" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Installing…" })).toBeInTheDocument());
    fireEvent.submit(screen.getByRole("form"));
    await waitFor(() => expect(contracts.createSchedulerBinding).toHaveBeenCalledTimes(1));
  });

  it("stops the install check when the dialog closes before it settles", async () => {
    let settle: (rows: unknown[]) => void = () => undefined;
    contracts.listProjectDetails.mockResolvedValue([project({ id: ALPHA, name: "Alpha" })]);
    contracts.listSchedulerBindings.mockReturnValue(
      new Promise<unknown[]>((resolve) => {
        settle = resolve;
      })
    );
    function Host(): React.ReactElement {
      const [current, setCurrent] = React.useState<Scheduler | null>(SCHEDULER);
      return <BulkInstallDialog workspaceSlug={SLUG} scheduler={current} onClose={() => setCurrent(null)} />;
    }
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const user = userEvent.setup();
    render(
      <QueryClientProvider client={queryClient}>
        <Host />
      </QueryClientProvider>
    );
    await openPicker(user);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
    settle([]);
  });

  it("shows the install check while target detection runs", async () => {
    contracts.listProjectDetails.mockResolvedValue([project({ id: ALPHA, name: "Alpha" })]);
    contracts.listSchedulerBindings.mockReturnValue(new Promise(() => undefined));
    const { user } = renderDialog();
    const listbox = await openPicker(user);
    expect(within(listbox).getByText("Checking installs…")).toBeInTheDocument();
  });

  it("notes an unreadable start time before writing and keeps the dialog open", async () => {
    contracts.listProjectDetails.mockResolvedValue([project({ id: ALPHA, name: "Alpha" })]);
    const { user, onClose } = renderDialog();
    const listbox = await openPicker(user);
    await waitFor(() => expect(within(listbox).getByLabelText(/Alpha/)).not.toBeDisabled());
    await user.click(within(listbox).getByLabelText(/Alpha/));
    await user.clear(screen.getByLabelText("Starts at"));
    await user.click(screen.getByRole("button", { name: "Install" }));
    expect(await screen.findByText("Start time is required.")).toBeInTheDocument();
    expect(contracts.createSchedulerBinding).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("names a failing target by its row, or its id when the row is gone", () => {
    const alpha = project({ id: ALPHA, name: "Alpha" });
    expect(projectName([alpha], ALPHA)).toBe("Alpha");
    expect(projectName([alpha], BETA)).toBe(BETA);
  });

  it("installs on every picked target and closes when all succeed", async () => {
    contracts.listProjectDetails.mockResolvedValue([
      project({ id: ALPHA, name: "Alpha" }),
      project({ id: BETA, name: "Beta" }),
    ]);
    contracts.createSchedulerBinding.mockResolvedValue(binding(ALPHA));
    const { user, onClose } = renderDialog();
    const listbox = await openPicker(user);
    await waitFor(() => expect(within(listbox).getByLabelText(/Alpha/)).not.toBeDisabled());

    await user.click(within(listbox).getByLabelText(/Alpha/));
    await user.click(within(listbox).getByLabelText(/Beta/));
    await user.click(screen.getByRole("button", { name: "Install" }));

    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(contracts.createSchedulerBinding).toHaveBeenCalledTimes(2);
    expect(contracts.createSchedulerBinding).toHaveBeenCalledWith(
      expect.anything(),
      SLUG,
      ALPHA,
      expect.objectContaining({ scheduler: "s1", rrule: expect.any(String), enabled: true })
    );
    expect(notifySuccess).toHaveBeenCalledWith("Installed on 2 projects", expect.any(String));
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("reports failures, keeps them selected and locks the successes for a retry", async () => {
    contracts.listProjectDetails.mockResolvedValue([
      project({ id: ALPHA, name: "Alpha" }),
      project({ id: BETA, name: "Beta" }),
    ]);
    contracts.createSchedulerBinding.mockImplementation((_client: unknown, _slug: string, projectId: string) =>
      projectId === BETA
        ? Promise.reject(
            new ApiError({ status: 400, code: "invalid", message: "Bad", fields: { rrule: ["Bad rule"] } })
          )
        : Promise.resolve(binding(projectId))
    );
    const { user, onClose } = renderDialog();
    const listbox = await openPicker(user);
    await waitFor(() => expect(within(listbox).getByLabelText(/Alpha/)).not.toBeDisabled());

    await user.click(within(listbox).getByLabelText(/Alpha/));
    await user.click(within(listbox).getByLabelText(/Beta/));
    await user.click(screen.getByRole("button", { name: "Install" }));

    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("1 project failed", "Beta — Bad rule"));
    expect(notifySuccess).toHaveBeenCalledWith("Installed on 1 project", expect.any(String));
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: /project/ })).toHaveTextContent("1 project selected");
    const reopened = await openPicker(user);
    await waitFor(() => expect(within(reopened).getByLabelText(/Beta/)).toBeChecked());
    expect(within(reopened).getByLabelText(/Alpha/)).toBeDisabled();
    expect(screen.getByRole("button", { name: /project/ })).toHaveTextContent("1 project selected");

    contracts.createSchedulerBinding.mockReset();
    contracts.createSchedulerBinding.mockResolvedValue(binding(BETA));
    await user.click(screen.getByRole("button", { name: "Install" }));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(contracts.createSchedulerBinding).toHaveBeenCalledTimes(1);
    expect(contracts.createSchedulerBinding).toHaveBeenCalledWith(expect.anything(), SLUG, BETA, expect.anything());
  });

  it("explains when the caller has joined no project", async () => {
    contracts.listProjectDetails.mockResolvedValue([project({ id: ALPHA, member_role: null })]);
    const { user } = renderDialog();
    expect(
      await screen.findByText("You haven't joined a project in this workspace to install on.")
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Install" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
  });

  it("closes on Escape without writing", async () => {
    contracts.listProjectDetails.mockResolvedValue([project({ id: ALPHA })]);
    const { user, onClose } = renderDialog();
    expect((await screen.findAllByText("Install Weekly review")).length).toBeGreaterThan(0);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(contracts.createSchedulerBinding).not.toHaveBeenCalled();
  });

  it("lets the cancel button close without writing", async () => {
    contracts.listProjectDetails.mockResolvedValue([project({ id: ALPHA })]);
    const { user, onClose } = renderDialog();
    expect((await screen.findAllByText("Install Weekly review")).length).toBeGreaterThan(0);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalled();
    expect(contracts.createSchedulerBinding).not.toHaveBeenCalled();
  });
});
