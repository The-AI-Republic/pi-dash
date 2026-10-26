// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ProjectDetails, SchedulerBinding } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { sessionKeys } from "../../core/session/queries.js";
import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER } from "../../core/session/index.js";
import { authKeys, projectRolesQueryOptions } from "../auth/index.js";
import { projectKeys } from "../projects/api/keys.js";

const { contracts } = vi.hoisted(() => ({
  contracts: { listSchedulerBindings: vi.fn(), updateSchedulerBinding: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});
vi.mock("../../shared/shell/toaster.js", () => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));

import { notifyError } from "../../shared/shell/toaster.js";
import { InstallsList, formatRunTimestamp, scheduleText, type InstallsListProps } from "./InstallsList.js";

const SLUG = "acme";
const PROJECT = "p1";

function binding(overrides: Partial<SchedulerBinding> = {}): SchedulerBinding {
  return {
    id: "b1",
    scheduler: "s1",
    scheduler_slug: "weekly-review",
    scheduler_name: "Weekly review",
    scheduler_color: "#22c55e",
    project: PROJECT,
    workspace: "w1",
    dtstart: "2026-10-20T09:00:00Z",
    tzid: "UTC",
    rrule: "FREQ=WEEKLY;BYDAY=MO",
    rdates: [],
    exdates: [],
    extra_context: "",
    enabled: true,
    outcome_mode: "create_issue",
    pod: null,
    pod_name: null,
    next_run_at: "2026-10-20T09:00:00Z",
    last_run: null,
    last_run_status: null,
    last_run_ended_at: null,
    last_error: "",
    actor: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-02T12:30:00Z",
    ...overrides,
  };
}

function project(overrides: Partial<ProjectDetails> = {}): ProjectDetails {
  return {
    id: PROJECT,
    created_at: "2026-09-01T00:00:00Z",
    updated_at: "2026-09-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: "w1",
    name: "Rocket",
    description: "",
    identifier: "RKT",
    sort_order: null,
    logo_props: {},
    member_role: 20,
    intake_count: 0,
    archived_at: null,
    cycle_view: true,
    issue_views_view: true,
    module_view: true,
    page_view: true,
    inbox_view: true,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 1,
    estimate: null,
    agent_executor_options: [],
    default_agent_executor: "",
    ...overrides,
  } as ProjectDetails;
}

function setup(options: {
  role: number;
  projectRole: number | null;
  bindings?: SchedulerBinding[];
  pending?: boolean;
  fail?: boolean;
  notAuthorized?: boolean;
  props?: Partial<InstallsListProps>;
}) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  queryClient.setQueryData(sessionKeys.workspaces, [
    { id: "w1", name: "Acme Workspace", slug: SLUG, logo_url: null, total_members: 3, role: options.role },
  ]);
  const roleMap: Record<string, number> =
    options.notAuthorized === true || options.projectRole === null ? {} : { [PROJECT]: options.projectRole };
  queryClient.setQueryData(authKeys.projectRoles(SLUG), roleMap);
  queryClient.setQueryData(projectKeys.details(SLUG), [project()]);
  queryClient.setQueryData(projectRolesQueryOptions({} as never, SLUG).queryKey, roleMap);
  if (options.pending === true) {
    contracts.listSchedulerBindings.mockReturnValue(new Promise(() => undefined));
  } else if (options.fail === true) {
    contracts.listSchedulerBindings.mockRejectedValue(new Error("Offline"));
  } else {
    contracts.listSchedulerBindings.mockResolvedValue(options.bindings ?? [binding()]);
  }
  const props: InstallsListProps = {
    workspaceSlug: SLUG,
    projectId: PROJECT,
    onOpenInstall: vi.fn(),
    onCreate: vi.fn(),
    ...options.props,
  };
  render(
    <QueryClientProvider client={queryClient}>
      <InstallsList {...props} />
    </QueryClientProvider>
  );
  return { queryClient, props, user: userEvent.setup() };
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});
afterEach(() => {
  cleanup();
  resetAppClient();
});

describe("formatters", () => {
  it("shows (never) for missing or invalid timestamps", () => {
    expect(formatRunTimestamp(null)).toBe("(never)");
    expect(formatRunTimestamp("")).toBe("(never)");
    expect(formatRunTimestamp("garbage")).toBe("(never)");
    expect(formatRunTimestamp("2026-10-02T12:30:00Z")).not.toBe("(never)");
  });

  it("renders the readable schedule", () => {
    expect(scheduleText({ rrule: "FREQ=WEEKLY;BYDAY=MO" })).toBe("every week on Monday");
    expect(scheduleText({ rrule: "" })).toBe("Fires once at the start time");
  });
});

describe("InstallsList", () => {
  it("lists installs with schedule, runs, switch and update time", async () => {
    setup({ role: ROLE_ADMIN, projectRole: 20 });
    const table = await screen.findByRole("table", { name: "Installed schedulers" });
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((cell) => cell.textContent)
    ).toEqual(["Scheduler", "Schedule", "Next run", "Last run", "Status", "Updated", ""]);
    const row = await screen.findByText("Weekly review");
    const tr = row.closest("tr");
    if (tr === null) throw new Error("row missing");
    expect(within(tr).getByText("weekly-review")).toBeInTheDocument();
    expect(within(tr).getByText("every week on Monday")).toBeInTheDocument();
    expect(within(tr).getByTitle("FREQ=WEEKLY;BYDAY=MO")).toBeInTheDocument();
    expect(within(tr).getAllByText("(never)")).toHaveLength(1);
    expect(within(tr).getByText("Enabled")).toBeInTheDocument();
    expect(within(tr).getByRole("switch", { name: "Disable scheduler" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("button", { name: "New Scheduler" })).toBeInTheDocument();
    expect(screen.queryByText("No schedulers installed on this project yet")).not.toBeInTheDocument();
  });

  it("opens the install detail from the row and never from the switch", async () => {
    contracts.updateSchedulerBinding.mockResolvedValue(binding({ enabled: false }));
    const { props, user } = setup({ role: ROLE_ADMIN, projectRole: 20 });
    await screen.findByText("Weekly review");
    await user.click(screen.getByText("Weekly review"));
    expect(props.onOpenInstall).toHaveBeenCalledWith(expect.objectContaining({ id: "b1" }));
    await user.click(screen.getByRole("switch", { name: "Disable scheduler" }));
    expect(props.onOpenInstall).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(contracts.updateSchedulerBinding).toHaveBeenCalled());
  });

  it("flips the switch optimistically and gates a second click while in flight", async () => {
    let settle: (value: SchedulerBinding) => void = () => undefined;
    contracts.updateSchedulerBinding.mockReturnValue(
      new Promise<SchedulerBinding>((resolve) => {
        settle = resolve;
      })
    );
    const { user } = setup({ role: ROLE_ADMIN, projectRole: 20 });
    await screen.findByText("Weekly review");
    await user.click(screen.getByRole("switch", { name: "Disable scheduler" }));
    await waitFor(() => expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "false"));
    expect(screen.getByRole("switch")).toHaveAttribute("aria-disabled", "true");
    await user.click(screen.getByRole("switch"));
    expect(contracts.updateSchedulerBinding).toHaveBeenCalledTimes(1);
    settle(binding({ enabled: false }));
    await waitFor(() => expect(screen.getByRole("switch")).not.toHaveAttribute("aria-disabled"));
  });

  it("gates each row's switch on its own write", async () => {
    const pending: Array<(value: SchedulerBinding) => void> = [];
    contracts.updateSchedulerBinding.mockImplementation(
      () =>
        new Promise<SchedulerBinding>((resolve) => {
          pending.push(resolve);
        })
    );
    const { user } = setup({
      role: ROLE_ADMIN,
      projectRole: 20,
      bindings: [binding(), binding({ id: "b2", scheduler_slug: "daily-triage", scheduler_name: "Daily triage" })],
    });
    await screen.findByText("Daily triage");
    const rowA = screen.getByText("Weekly review").closest("tr");
    const rowB = screen.getByText("Daily triage").closest("tr");
    if (rowA === null || rowB === null) throw new Error("rows missing");
    const switchA = within(rowA).getByRole("switch", { name: "Disable scheduler" });
    const switchB = within(rowB).getByRole("switch", { name: "Disable scheduler" });
    await user.click(switchA);
    await user.click(switchB);
    expect(switchA).toHaveAttribute("aria-disabled", "true");
    expect(switchB).toHaveAttribute("aria-disabled", "true");
    await user.click(within(rowA).getByRole("switch"));
    expect(contracts.updateSchedulerBinding).toHaveBeenCalledTimes(2);
    pending.forEach((settle) => settle(binding({ enabled: false })));
    await waitFor(() =>
      expect(
        within(screen.getByText("Weekly review").closest("tr") as HTMLElement).getByRole("switch")
      ).not.toHaveAttribute("aria-disabled")
    );
    expect(
      within(screen.getByText("Daily triage").closest("tr") as HTMLElement).getByRole("switch")
    ).not.toHaveAttribute("aria-disabled");
  });

  it("reports a refused flip", async () => {
    contracts.updateSchedulerBinding.mockRejectedValue(new Error("Admins only"));
    const { user } = setup({ role: ROLE_ADMIN, projectRole: 20 });
    await screen.findByText("Weekly review");
    await user.click(screen.getByRole("switch", { name: "Disable scheduler" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Something went wrong", "Admins only"));
  });

  it.each([ROLE_MEMBER, ROLE_GUEST])("reads only for workspace role %s", async (role) => {
    setup({ role, projectRole: 15 });
    await screen.findByText("Weekly review");
    expect(screen.queryByRole("button", { name: "New Scheduler" })).not.toBeInTheDocument();
    expect(screen.getByRole("switch")).toHaveAttribute("aria-disabled", "true");
  });

  it("refuses management when the roles read failed", async () => {
    setup({ role: ROLE_ADMIN, projectRole: null, notAuthorized: true });
    await screen.findByText("Weekly review");
    expect(screen.queryByRole("button", { name: "New Scheduler" })).not.toBeInTheDocument();
    expect(screen.getByRole("switch")).toHaveAttribute("aria-disabled", "true");
  });

  it("treats a project missing from the role map as a non-admin", async () => {
    setup({ role: ROLE_ADMIN, projectRole: 20, notAuthorized: true });
    await screen.findByText("Weekly review");
    expect(screen.queryByRole("button", { name: "New Scheduler" })).not.toBeInTheDocument();
  });

  it("shows only the actions the caller supplies", async () => {
    const onEditInstall = vi.fn();
    const { user } = setup({ role: ROLE_ADMIN, projectRole: 20, props: { onEditInstall } });
    await screen.findByText("Weekly review");
    await user.click(screen.getByRole("button", { name: "Edit" }));
    expect(onEditInstall).toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Uninstall" })).not.toBeInTheDocument();
  });

  it("invites creation on an empty list and hides the copy from members", async () => {
    setup({ role: ROLE_ADMIN, projectRole: 20, bindings: [] });
    expect(await screen.findByText("No schedulers installed on this project yet")).toBeInTheDocument();
    expect(screen.getByText(/Create one here/)).toBeInTheDocument();
    cleanup();
    setup({ role: ROLE_MEMBER, projectRole: 15, bindings: [] });
    expect(await screen.findByText(/project admin can install/)).toBeInTheDocument();
  });

  it("calls the creation handler and renders the action slot when supplied", async () => {
    const onEditInstall = vi.fn();
    const onUninstallInstall = vi.fn();
    const { props, user } = setup({
      role: ROLE_ADMIN,
      projectRole: 20,
      props: { onEditInstall, onUninstallInstall },
    });
    await screen.findByText("Weekly review");
    await user.click(screen.getByRole("button", { name: "New Scheduler" }));
    expect(props.onCreate).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole("button", { name: "Edit" }));
    expect(onEditInstall).toHaveBeenCalledWith(expect.objectContaining({ id: "b1" }));
    await user.click(screen.getByRole("button", { name: "Uninstall" }));
    expect(onUninstallInstall).toHaveBeenCalledWith(expect.objectContaining({ id: "b1" }));
    expect(props.onOpenInstall).not.toHaveBeenCalled();
  });

  it("renders a skeleton, then the failure with a retry", async () => {
    contracts.listSchedulerBindings.mockReturnValueOnce(new Promise(() => undefined));
    const { user } = setup({ role: ROLE_ADMIN, projectRole: 20, pending: true });
    expect(screen.getByRole("status", { name: "Loading installs" })).toBeInTheDocument();
    cleanup();
    contracts.listSchedulerBindings.mockRejectedValueOnce(new Error("Offline")).mockResolvedValueOnce([]);
    setup({ role: ROLE_ADMIN, projectRole: 20, fail: true });
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Offline"));
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("No schedulers installed on this project yet")).toBeInTheDocument();
  });

  it("uses the settings variant copy and falls back to the slug in the title", async () => {
    setup({ role: ROLE_ADMIN, projectRole: 20, props: { variant: "settings" } });
    await screen.findByText("Weekly review");
    expect(screen.getByText(/Scheduled instructions installed on this project/)).toBeInTheDocument();
  });

  it("names the creation control in the settings empty row", async () => {
    setup({ role: ROLE_ADMIN, projectRole: 20, props: { variant: "settings" }, bindings: [] });
    expect(await screen.findByText("No schedulers installed on this project yet")).toBeInTheDocument();
    expect(
      screen.getByText("Use New Scheduler to install a catalog definition or write a new one.")
    ).toBeInTheDocument();
  });

  it("falls back to the slug when the project row is unknown", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(sessionKeys.workspaces, []);
    queryClient.setQueryData(projectRolesQueryOptions({} as never, SLUG).queryKey, { [PROJECT]: 20 });
    contracts.listSchedulerBindings.mockResolvedValue([binding()]);
    render(
      <QueryClientProvider client={queryClient}>
        <InstallsList workspaceSlug={SLUG} projectId={PROJECT} onOpenInstall={vi.fn()} onCreate={vi.fn()} />
      </QueryClientProvider>
    );
    await screen.findByText("Weekly review");
    expect(document.title).toBe("acme · Schedulers");
  });
});
