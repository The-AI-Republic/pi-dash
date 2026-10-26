// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Install detail page (parity/specs/project-settings-part-f.md, PSET-059):
// configuration grid, latest-error banner, collapsible composed prompt,
// admin controls, paged run history with its empty states, and the
// not-available notice with a way back.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { AgentRunPage, SchedulerBindingDetail } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ROLE_ADMIN, sessionKeys } from "../../core/session/index.js";
import { createAppClient, resetAppClient } from "../../core/api/client.js";

const { contracts, navigate } = vi.hoisted(() => ({
  contracts: {
    deleteSchedulerBinding: vi.fn(),
    getSchedulerBinding: vi.fn(),
    listBindingRuns: vi.fn(),
    listWorkspaceMembers: vi.fn(),
    updateSchedulerBinding: vi.fn(),
  },
  navigate: vi.fn(),
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});
vi.mock("../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
  useDocumentHead: vi.fn(),
}));
vi.mock("@tanstack/react-router", () => ({
  useNavigate: () => navigate,
  Link: (props: {
    children?: React.ReactNode;
    to?: string;
    className?: string;
    params?: Record<string, string>;
    onClick?: (event: { stopPropagation: () => void }) => void;
  }) => {
    const href = Object.entries(props.params ?? {}).reduce(
      (path, [key, value]) => path.replace(`$${key}`, value),
      String(props.to)
    );
    return (
      <a href={href} className={props.className} data-params={JSON.stringify(props.params)} onClick={props.onClick}>
        {props.children as React.ReactNode}
      </a>
    );
  },
}));

import { notifyError, useDocumentHead } from "../../shared/shell/index.js";
import { BindingDetailBody, BindingDetailPage } from "./BindingDetailPage.js";

const SLUG = "acme";
const PROJECT = "p1";
const BINDING = "b1";

function binding(overrides: Partial<SchedulerBindingDetail> = {}): SchedulerBindingDetail {
  return {
    id: BINDING,
    scheduler: "s1",
    scheduler_slug: "weekly-review",
    scheduler_name: "Weekly review",
    scheduler_color: "#22c55e",
    project: PROJECT,
    workspace: "w1",
    dtstart: "2026-10-20T09:00:00Z",
    tzid: "Europe/Paris",
    rrule: "FREQ=WEEKLY;BYDAY=FR",
    rdates: [],
    exdates: [],
    extra_context: "Focus on the API.",
    enabled: true,
    outcome_mode: "apply_fix",
    pod: null,
    pod_name: null,
    next_run_at: "2026-10-23T09:00:00Z",
    last_run: null,
    last_run_status: null,
    last_run_ended_at: null,
    last_error: "",
    actor: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-02T12:30:00Z",
    resolved_prompt: "Audit the project.\n\nFocus on the API.",
    run_count: 0,
    scheduler_source: "builtin",
    scheduler_is_enabled: true,
    ...overrides,
  } as SchedulerBindingDetail;
}

function runRow(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: "r1",
    status: "completed",
    error: "",
    done_payload: { summary: "Filed one issue." },
    started_at: "2026-10-20T09:00:00Z",
    ended_at: "2026-10-20T09:02:00Z",
    created_at: "2026-10-20T08:59:00Z",
    pod_detail: { id: "pod1", name: "Main pool" },
    ...overrides,
  };
}

function page(results: Record<string, unknown>[], overrides: Partial<AgentRunPage> = {}): AgentRunPage {
  return {
    results,
    count: results.length,
    total_count: results.length,
    total_pages: 1,
    page: 1,
    per_page: 25,
    ...overrides,
  } as unknown as AgentRunPage;
}

function setup(options: {
  detail?: SchedulerBindingDetail | Error;
  runs?: AgentRunPage | Error;
  canManage?: boolean;
  canSeeWorkspaceDefinition?: boolean;
}) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  if (options.detail instanceof Error) {
    contracts.getSchedulerBinding.mockRejectedValue(options.detail);
  } else {
    contracts.getSchedulerBinding.mockResolvedValue(options.detail ?? binding());
  }
  if (options.runs instanceof Error) {
    contracts.listBindingRuns.mockRejectedValue(options.runs);
  } else {
    contracts.listBindingRuns.mockResolvedValue(options.runs ?? page([]));
  }
  render(
    <QueryClientProvider client={queryClient}>
      <BindingDetailBody
        workspaceSlug={SLUG}
        projectId={PROJECT}
        bindingId={BINDING}
        canManage={options.canManage ?? true}
        canSeeWorkspaceDefinition={options.canSeeWorkspaceDefinition ?? true}
      />
    </QueryClientProvider>
  );
  return { queryClient, user: userEvent.setup() };
}

beforeEach(() => {
  vi.clearAllMocks();
  contracts.listWorkspaceMembers.mockResolvedValue([]);
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});
afterEach(() => {
  cleanup();
  resetAppClient();
});

describe("BindingDetailBody", () => {
  it("shows the header marks, config grid and the workspace definition link", async () => {
    setup({});
    expect(await screen.findByRole("heading", { level: 1, name: "Weekly review" })).toBeInTheDocument();
    expect(screen.getByText("weekly-review")).toBeInTheDocument();
    expect(screen.getByText("builtin")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "View workspace definition" })).toBeInTheDocument();
    const grid = screen.getByRole("heading", { name: "Configuration" }).nextElementSibling;
    if (grid === null) throw new Error("grid missing");
    const labels = within(grid as HTMLElement)
      .getAllByRole("term")
      .map((term) => term.textContent);
    expect(labels).toContain("Schedule");
    expect(labels).toContain("Time zone");
    expect(labels).toContain("Pod");
    expect(labels).toContain("Enabled");
    expect(within(grid as HTMLElement).getByText("Europe/Paris")).toBeInTheDocument();
    expect(within(grid as HTMLElement).getByText("(default pod)")).toBeInTheDocument();
    expect(within(grid as HTMLElement).getByText("apply_fix")).toBeInTheDocument();
  });

  it("names the installer from the workspace directory, falling back to the id", async () => {
    contracts.listWorkspaceMembers.mockResolvedValue([
      {
        id: "m1",
        member: {
          id: "u1",
          first_name: "Ada",
          last_name: "Byron",
          avatar: "",
          avatar_url: null,
          display_name: "Ada B",
          is_bot: false,
          email: "ada@example.test",
        },
        role: 20,
      },
    ]);
    setup({ detail: binding({ actor: "u1" }) });
    const grid = (await screen.findByRole("heading", { name: "Configuration" })).nextElementSibling;
    if (grid === null) throw new Error("grid missing");
    expect(within(grid as HTMLElement).getByText("Installed by")).toBeInTheDocument();
    expect(within(grid as HTMLElement).getByText("Ada B")).toBeInTheDocument();
  });

  it("reveals and hides the composed prompt", async () => {
    const { user } = setup({});
    const toggle = await screen.findByRole("button", { name: "Show resolved prompt" });
    expect(screen.queryByText(/Audit the project/)).not.toBeInTheDocument();
    await user.click(toggle);
    expect(screen.getByText(/Audit the project/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Hide resolved prompt" }));
    expect(screen.queryByText(/Audit the project/)).not.toBeInTheDocument();
  });

  it("surfaces the latest error and the project context", async () => {
    setup({ detail: binding({ last_error: "Runner refused the worktree." }) });
    expect(await screen.findByText("Last error")).toBeInTheDocument();
    expect(screen.getByText("Runner refused the worktree.")).toBeInTheDocument();
    expect(screen.getByText("Focus on the API.")).toBeInTheDocument();
  });

  it("hides admin-only controls and the workspace link for a member", async () => {
    setup({ canManage: false, canSeeWorkspaceDefinition: false });
    await screen.findByRole("heading", { level: 1, name: "Weekly review" });
    expect(screen.queryByRole("button", { name: "Edit" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Uninstall" })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "View workspace definition" })).not.toBeInTheDocument();
    expect(screen.getByRole("switch")).toHaveAttribute("aria-disabled", "true");
  });

  it("badges a workspace-wide halt", async () => {
    setup({ detail: binding({ scheduler_is_enabled: false }) });
    expect(await screen.findByText("Disabled for the whole workspace")).toBeInTheDocument();
  });

  it("still renders the detail for a workspace-disabled definition", async () => {
    setup({ detail: binding({ scheduler_is_enabled: false, rrule: "" }) });
    expect(await screen.findByRole("heading", { name: "Configuration" })).toBeInTheDocument();
    expect(screen.getByText("Disabled for the whole workspace")).toBeInTheDocument();
    expect(screen.queryByText(/This scheduler install is not available/)).not.toBeInTheDocument();
    const grid = screen.getByRole("heading", { name: "Configuration" }).nextElementSibling;
    if (grid === null) throw new Error("grid missing");
    expect(within(grid as HTMLElement).getByText("Fires once at the start time")).toBeInTheDocument();
  });

  it("badges a completed finite series once reported", async () => {
    setup({ detail: binding({ series_exhausted: true } as Partial<SchedulerBindingDetail>) });
    expect(await screen.findByText("Completed")).toBeInTheDocument();
  });

  it("names the next run in the empty history", async () => {
    setup({ runs: page([]) });
    expect(await screen.findByText(/No runs yet — next run at/)).toBeInTheDocument();
    expect(screen.getByText("0 runs")).toBeInTheDocument();
    expect(screen.queryByText(/^Page \d+ of \d+$/)).not.toBeInTheDocument();
  });

  it("names the disabled empty state", async () => {
    setup({ detail: binding({ enabled: false }) });
    expect(await screen.findByText(/Scheduler is disabled/)).toBeInTheDocument();
  });

  it("lists run rows and opens the run detail", async () => {
    const { user } = setup({ runs: page([runRow()]) });
    const history = await screen.findByRole("region", { name: "Run history" });
    expect(within(history).getByText("Completed")).toBeInTheDocument();
    expect(within(history).getByText("2m 0s")).toBeInTheDocument();
    expect(within(history).getByText("Main pool")).toBeInTheDocument();
    expect(within(history).getByText("Filed one issue.")).toBeInTheDocument();
    await user.click(within(history).getByText("Main pool"));
    expect(navigate).toHaveBeenCalledWith({ href: `/${SLUG}/projects/${PROJECT}/runners/runs/r1` });
  });

  it("offers each run row as a link to the run detail", async () => {
    setup({ runs: page([runRow()]) });
    const history = await screen.findByRole("region", { name: "Run history" });
    // The Started cell renders in the machine's own timezone, so the row is
    // found by its timezone-independent pod cell instead of the link text.
    const row = within(history).getByRole("row", { name: /Main pool/ });
    expect(within(row).getByRole("link")).toHaveAttribute("href", `/${SLUG}/projects/${PROJECT}/runners/runs/r1`);
  });

  it("keeps the run link from firing the row navigation", async () => {
    const { user } = setup({ runs: page([runRow()]) });
    const history = await screen.findByRole("region", { name: "Run history" });
    const row = within(history).getByRole("row", { name: /Main pool/ });
    await user.click(within(row).getByRole("link"));
    expect(navigate).not.toHaveBeenCalled();
  });

  it("pages through the run history", async () => {
    const { user } = setup({
      runs: page([runRow()], { total_count: 40, total_pages: 2, page: 1 }),
    });
    const history = await screen.findByRole("region", { name: "Run history" });
    expect(within(history).getByText("Page 1 of 2")).toBeInTheDocument();
    expect(within(history).getByRole("button", { name: "Previous" })).toBeDisabled();
    await user.click(within(history).getByRole("button", { name: "Next" }));
    expect(within(history).getByText("Page 2 of 2")).toBeInTheDocument();
    expect(within(history).getByRole("button", { name: "Next" })).toBeDisabled();
    await user.click(within(history).getByRole("button", { name: "Previous" }));
    expect(within(history).getByText("Page 1 of 2")).toBeInTheDocument();
  });

  it("flips enablement through the config switch", async () => {
    contracts.updateSchedulerBinding.mockResolvedValue(binding({ enabled: false }));
    const { user } = setup({});
    const toggle = await screen.findByRole("switch", { name: "Disable scheduler" });
    await user.click(toggle);
    await waitFor(() =>
      expect(contracts.updateSchedulerBinding).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, BINDING, {
        enabled: false,
      })
    );
  });

  it("flips the switch optimistically before the round trip lands", async () => {
    let release: (value: SchedulerBindingDetail) => void = () => undefined;
    contracts.updateSchedulerBinding.mockReturnValue(
      new Promise<SchedulerBindingDetail>((resolve) => {
        release = resolve;
      })
    );
    const { user, queryClient } = setup({});
    const toggle = await screen.findByRole("switch", { name: "Disable scheduler" });
    expect(toggle).toHaveAttribute("aria-checked", "true");
    await user.click(toggle);
    await waitFor(() => expect(contracts.updateSchedulerBinding).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(
        queryClient.getQueryData<{ enabled: boolean }>(["ws", SLUG, "scheduler-bindings", PROJECT, "detail", BINDING])
          ?.enabled
      ).toBe(false)
    );
    release(binding({ enabled: false }));
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "false"));
    expect(toggle).toBeEnabled();
    const grid = screen.getByRole("heading", { name: "Configuration" }).nextElementSibling;
    if (grid === null) throw new Error("grid missing");
    expect(within(grid as HTMLElement).getByText("Disabled")).toBeInTheDocument();
  });

  it("rolls the switch back when the server refuses the flip", async () => {
    contracts.updateSchedulerBinding.mockRejectedValue(new Error("Admins only"));
    const { user } = setup({});
    const toggle = await screen.findByRole("switch", { name: "Disable scheduler" });
    await user.click(toggle);
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "true"));
    expect(notifyError).toHaveBeenCalledWith("Something went wrong", "Admins only");
  });

  it("opens the uninstall confirmation from the header", async () => {
    const { user } = setup({});
    await user.click(await screen.findByRole("button", { name: "Uninstall" }));
    expect(await screen.findByRole("heading", { name: "Uninstall scheduler?" })).toBeInTheDocument();
    expect(screen.getByText(/definition itself is unaffected/)).toBeInTheDocument();
  });

  it("shows a skeleton while the install read is unresolved", () => {
    contracts.getSchedulerBinding.mockReturnValue(new Promise(() => undefined));
    contracts.listBindingRuns.mockReturnValue(new Promise(() => undefined));
    setup({ detail: binding() });
    expect(screen.getByRole("status", { name: "Loading scheduler install" })).toBeInTheDocument();
  });

  it("hides the context and prompt blocks when the install carries neither", async () => {
    setup({ detail: binding({ extra_context: "", resolved_prompt: "" }) });
    await screen.findByRole("heading", { level: 1, name: "Weekly review" });
    expect(screen.queryByText("Project context")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Show resolved prompt" })).not.toBeInTheDocument();
  });

  it("reads a failed run's error as its result excerpt and leaves a passing one quiet", async () => {
    setup({
      runs: page([
        runRow({ id: "r1", status: "failed", error: "The pool went away.", done_payload: null }),
        runRow({ id: "r2", status: "completed", done_payload: { summary: "All clear." } }),
      ]),
    });
    const history = await screen.findByRole("region", { name: "Run history" });
    expect(within(history).getByText("The pool went away.")).toBeInTheDocument();
    expect(within(history).getByText("All clear.")).toBeInTheDocument();
  });

  it("renders with an empty directory read and no excerpt for a quiet run", async () => {
    contracts.listWorkspaceMembers.mockReturnValue(new Promise(() => undefined));
    setup({
      runs: page([runRow({ id: "r3", status: "completed", error: "", done_payload: null })]),
    });
    const history = await screen.findByRole("region", { name: "Run history" });
    expect(within(history).getByText("Completed")).toBeInTheDocument();
    expect(within(history).queryByTitle(/./)).not.toBeInTheDocument();
  });

  it("re-reads the install after a saved edit", async () => {
    const { user } = setup({});
    contracts.getSchedulerBinding.mockClear();
    contracts.updateSchedulerBinding.mockResolvedValue(binding());
    await user.click(await screen.findByRole("button", { name: "Edit" }));
    await user.click(await screen.findByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(contracts.getSchedulerBinding).toHaveBeenCalled());
  });

  it("returns to the list after uninstalling from the detail page", async () => {
    const { user } = setup({});
    contracts.deleteSchedulerBinding.mockResolvedValue(undefined);
    await user.click(await screen.findByRole("button", { name: "Uninstall" }));
    const dialog = await screen.findByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "Uninstall" }));
    await waitFor(() =>
      expect(navigate).toHaveBeenCalledWith({ href: `/${SLUG}/projects/${PROJECT}/schedulers/list` })
    );
  });

  it("shows the not-available notice with a back link when the install is gone", async () => {
    setup({ detail: new Error("404") });
    expect(await screen.findByText(/This scheduler install is not available/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /Back to schedulers/ })).toBeInTheDocument();
  });

  it("explains an unreadable run history as a possibly removed install with a way back", async () => {
    setup({ runs: new Error("404") });
    const history = await screen.findByRole("region", { name: "Run history" });
    const alert = within(history).getByRole("alert");
    expect(alert).toHaveTextContent("may have been removed");
    expect(within(alert).getByRole("link", { name: /Back to schedulers/ })).toBeInTheDocument();
  });
});

describe("BindingDetailPage", () => {
  it("hands the resolved admin flags to the body", async () => {
    contracts.getSchedulerBinding.mockResolvedValue(binding());
    contracts.listBindingRuns.mockResolvedValue(page([]));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(["ws", SLUG, "auth", "project-roles"], { [PROJECT]: 20 });
    queryClient.setQueryData(["ws", SLUG, "auth", "membership"], { role: ROLE_ADMIN });
    render(
      <QueryClientProvider client={queryClient}>
        <BindingDetailPage workspaceSlug={SLUG} projectId={PROJECT} bindingId={BINDING} />
      </QueryClientProvider>
    );
    expect(await screen.findByRole("button", { name: "Edit" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "View workspace definition" })).toBeInTheDocument();
  });

  it("withholds admin controls from a member and falls back to the slug in the title", async () => {
    contracts.getSchedulerBinding.mockResolvedValue(binding());
    contracts.listBindingRuns.mockResolvedValue(page([]));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(["ws", SLUG, "auth", "project-roles"], {});
    queryClient.setQueryData(["ws", SLUG, "auth", "membership"], null);
    render(
      <QueryClientProvider client={queryClient}>
        <BindingDetailPage workspaceSlug={SLUG} projectId={PROJECT} bindingId={BINDING} />
      </QueryClientProvider>
    );
    await screen.findByRole("heading", { level: 1, name: "Weekly review" });
    expect(screen.queryByRole("button", { name: "Edit" })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "View workspace definition" })).not.toBeInTheDocument();
    expect(vi.mocked(useDocumentHead)).toHaveBeenCalledWith({
      title: "acme · Scheduler install",
      robots: "noindex, nofollow",
    });
  });

  it("ignores a second switch flip while the first is still in flight", async () => {
    let release: (value: SchedulerBindingDetail) => void = () => undefined;
    contracts.updateSchedulerBinding.mockReturnValue(
      new Promise<SchedulerBindingDetail>((resolve) => {
        release = resolve;
      })
    );
    const { user } = setup({});
    const toggle = await screen.findByRole("switch", { name: "Disable scheduler" });
    await user.click(toggle);
    await user.click(toggle);
    await waitFor(() => expect(contracts.updateSchedulerBinding).toHaveBeenCalledTimes(1));
    release(binding({ enabled: false }));
  });

  it("names the workspace instead of the slug once the row is loaded", async () => {
    contracts.getSchedulerBinding.mockResolvedValue(binding());
    contracts.listBindingRuns.mockResolvedValue(page([]));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(sessionKeys.workspaces, [
      { id: "w1", name: "Acme Workspace", slug: SLUG, logo_url: null, total_members: 3, role: ROLE_ADMIN },
    ]);
    queryClient.setQueryData(["ws", SLUG, "auth", "project-roles"], { [PROJECT]: 20 });
    queryClient.setQueryData(["ws", SLUG, "auth", "membership"], { role: ROLE_ADMIN });
    render(
      <QueryClientProvider client={queryClient}>
        <BindingDetailPage workspaceSlug={SLUG} projectId={PROJECT} bindingId={BINDING} />
      </QueryClientProvider>
    );
    await screen.findByRole("heading", { level: 1, name: "Weekly review" });
    expect(vi.mocked(useDocumentHead)).toHaveBeenCalledWith(
      expect.objectContaining({ title: "Acme Workspace · Scheduler install" })
    );
  });

  it("waits on the standing reads before rendering", () => {
    contracts.getSchedulerBinding.mockReturnValue(new Promise(() => undefined));
    contracts.listBindingRuns.mockReturnValue(new Promise(() => undefined));
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <BindingDetailPage workspaceSlug={SLUG} projectId={PROJECT} bindingId={BINDING} />
      </QueryClientProvider>
    );
    expect(screen.getByRole("status", { name: "Loading scheduler install" })).toBeInTheDocument();
  });
});
