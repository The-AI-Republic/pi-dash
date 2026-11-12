/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, screen, fireEvent } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ISchedulerBinding } from "@pi-dash/services";
import type { IAgentRun, IAgentRunPage } from "@pi-dash/types";

const { useSWR, navigate, retrieveBinding, listBindingRuns, updateBinding } = vi.hoisted(() => ({
  useSWR: vi.fn(),
  navigate: vi.fn(),
  retrieveBinding: vi.fn(),
  listBindingRuns: vi.fn(),
  updateBinding: vi.fn(),
}));

vi.mock("swr", () => ({
  default: useSWR,
}));

vi.mock("react-router", () => ({
  useNavigate: () => navigate,
  Link: ({ to, children }: { to: string; children: React.ReactNode }) => <a href={to}>{children}</a>,
}));

vi.mock("mobx-react", () => ({
  observer: (component: unknown) => component,
}));

vi.mock("@pi-dash/i18n", () => ({
  useTranslation: () => ({
    t: (key: string, vars?: Record<string, unknown>) =>
      vars ? key.replace(/\{(\w+)\}/g, (_m, k) => String(vars[k] ?? "")) : key,
  }),
}));

vi.mock("@pi-dash/constants", () => ({
  API_BASE_URL: "http://localhost:8000",
  EUserPermissions: { ADMIN: 20 },
  EUserPermissionsLevel: { PROJECT: "PROJECT", WORKSPACE: "WORKSPACE" },
}));

vi.mock("@pi-dash/propel/toast", () => ({
  TOAST_TYPE: { ERROR: "error", SUCCESS: "success" },
  setToast: vi.fn(),
}));

vi.mock("@pi-dash/services", () => ({
  SchedulerService: class {
    retrieveBinding = retrieveBinding;
    listBindingRuns = listBindingRuns;
    updateBinding = updateBinding;
  },
}));

vi.mock("@pi-dash/ui", () => ({
  Badge: ({ children }: { children: React.ReactNode }) => <span data-testid="badge">{children}</span>,
  Button: ({ children, onClick }: { children: React.ReactNode; onClick?: () => void }) => (
    <button type="button" onClick={onClick}>
      {children}
    </button>
  ),
  ToggleSwitch: ({
    value,
    onChange,
    disabled,
  }: {
    value: boolean;
    onChange: (v: boolean) => void;
    disabled?: boolean;
  }) => <input type="checkbox" role="switch" checked={value} disabled={disabled} onChange={() => onChange(!value)} />,
}));

vi.mock("@/components/project/scheduler-bindings/edit-binding-modal", () => ({
  EditSchedulerBindingModal: ({ isOpen }: { isOpen: boolean }) => (isOpen ? <div data-testid="edit-modal" /> : null),
}));

vi.mock("@/components/project/scheduler-bindings/uninstall-binding-modal", () => ({
  UninstallSchedulerBindingModal: ({ isOpen }: { isOpen: boolean }) =>
    isOpen ? <div data-testid="uninstall-modal" /> : null,
}));

vi.mock("@/hooks/store/user", () => ({
  useUserPermissions: () => ({ allowPermissions: () => true }),
}));

vi.mock("@/hooks/store/use-member", () => ({
  useMember: () => ({ getUserDetails: (id: string) => ({ id, display_name: "Ada" }) }),
}));

import { SchedulerBindingDetail } from "../../core/components/project/scheduler-bindings/binding-detail";

function makeBinding(overrides: Partial<ISchedulerBinding> = {}): ISchedulerBinding {
  return {
    id: "binding-1",
    scheduler: "sched-1",
    scheduler_slug: "nightly-audit",
    scheduler_name: "Nightly Audit",
    scheduler_color: "#10b981",
    project: "proj-1",
    workspace: "ws-1",
    dtstart: "2026-09-01T09:00:00Z",
    tzid: "UTC",
    rrule: "FREQ=DAILY",
    rdates: [],
    exdates: [],
    extra_context: "Focus on the API.",
    enabled: true,
    outcome_mode: "create_issue",
    pod: null,
    pod_name: null,
    next_run_at: "2026-09-27T09:00:00Z",
    last_run: null,
    last_run_status: null,
    last_run_ended_at: null,
    last_error: "",
    actor: "user-1",
    created_at: "2026-09-01T00:00:00Z",
    updated_at: "2026-09-01T00:00:00Z",
    resolved_prompt: "Audit the project.\n\nFocus on the API.\n\nCreate issues.",
    run_count: 2,
    scheduler_source: "builtin",
    scheduler_is_enabled: true,
    ...overrides,
  };
}

function makeRun(): IAgentRun {
  return {
    id: "run-1",
    status: "completed",
    started_at: "2026-09-26T09:00:01Z",
    ended_at: "2026-09-26T09:03:01Z",
    created_at: "2026-09-26T09:00:00Z",
    error: "",
    done_payload: { summary: "Filed 2 issues" },
    pod_detail: { name: "WEB_pod_1" },
    scheduler_binding: "binding-1",
  } as unknown as IAgentRun;
}

function makeRunsPage(results: IAgentRun[], overrides: Partial<IAgentRunPage> = {}): IAgentRunPage {
  return {
    results,
    count: results.length,
    total_count: results.length,
    total_pages: 1,
    page: 1,
    per_page: 30,
    ...overrides,
  };
}

/** Route the two useSWR call sites (binding detail, run history) by key. */
function mockSwr(binding: ISchedulerBinding | undefined, runsPage: IAgentRunPage | undefined, bindingError?: unknown) {
  useSWR.mockImplementation((key: unknown) => {
    const kind = Array.isArray(key) ? key[0] : null;
    if (kind === "scheduler-binding-detail") return { data: binding, error: bindingError, mutate: vi.fn() };
    if (kind === "scheduler-binding-runs") return { data: runsPage, error: undefined, mutate: vi.fn() };
    return { data: undefined, error: undefined, mutate: vi.fn() };
  });
}

function renderDetail() {
  return render(<SchedulerBindingDetail workspaceSlug="acme" projectId="proj-1" bindingId="binding-1" />);
}

describe("SchedulerBindingDetail", () => {
  beforeEach(() => {
    useSWR.mockReset();
    navigate.mockReset();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("renders the binding config: name, slug, schedule, pod default and next run", () => {
    mockSwr(makeBinding(), makeRunsPage([makeRun()]));
    renderDetail();

    expect(screen.getByText("Nightly Audit")).toBeTruthy();
    expect(screen.getByText("nightly-audit")).toBeTruthy();
    expect(screen.getByText("(default pod)")).toBeTruthy();
    expect(screen.getByText("Focus on the API.")).toBeTruthy();
    expect(screen.getByText("Installed by")).toBeTruthy();
    expect(screen.getByText("Ada")).toBeTruthy();
  });

  it("reveals the resolved prompt behind a collapsible toggle", () => {
    mockSwr(makeBinding(), makeRunsPage([]));
    renderDetail();

    expect(screen.queryByText(/Audit the project\./)).toBeNull();
    fireEvent.click(screen.getByText("Show resolved prompt"));
    expect(screen.getByText(/Audit the project\./)).toBeTruthy();
    expect(screen.getByText("Hide resolved prompt")).toBeTruthy();
  });

  it("surfaces last_error prominently when non-empty", () => {
    mockSwr(makeBinding({ last_error: "no default pod configured" }), makeRunsPage([]));
    renderDetail();

    expect(screen.getByText("Last error")).toBeTruthy();
    expect(screen.getByText("no default pod configured")).toBeTruthy();
  });

  it("lists run history rows and navigates to the run detail view on click", () => {
    mockSwr(makeBinding(), makeRunsPage([makeRun()]));
    renderDetail();

    expect(screen.getByText("completed")).toBeTruthy();
    expect(screen.getByText("WEB_pod_1")).toBeTruthy();
    fireEvent.click(screen.getByText("Filed 2 issues"));
    expect(navigate).toHaveBeenCalledWith("/acme/projects/proj-1/runners/runs/run-1");
  });

  it("shows the next-run empty state for an enabled binding with no runs", () => {
    mockSwr(makeBinding(), makeRunsPage([]));
    renderDetail();

    expect(screen.getByText(/No runs yet — next run at/)).toBeTruthy();
  });

  it("shows the disabled empty state when the binding is off", () => {
    mockSwr(makeBinding({ enabled: false }), makeRunsPage([]));
    renderDetail();

    expect(screen.getByText("Scheduler is disabled — it will not fire until re-enabled.")).toBeTruthy();
  });

  it("shows a not-available message when the binding fetch fails", () => {
    mockSwr(undefined, undefined, { error: "not found" });
    renderDetail();

    expect(screen.getByText("This scheduler install is not available. It may have been uninstalled.")).toBeTruthy();
  });
});
