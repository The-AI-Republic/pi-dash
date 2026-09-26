/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, screen, fireEvent } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ISchedulerBinding } from "@pi-dash/services";

const { useSWR, navigate } = vi.hoisted(() => ({
  useSWR: vi.fn(),
  navigate: vi.fn(),
}));

vi.mock("swr", () => ({
  default: useSWR,
}));

vi.mock("react-router", () => ({
  useNavigate: () => navigate,
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
    listBindings = vi.fn();
    listSchedulers = vi.fn();
    updateBinding = vi.fn();
  },
}));

vi.mock("@pi-dash/ui", () => ({
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
  EditSchedulerBindingModal: () => null,
}));

vi.mock("@/components/project/scheduler-bindings/install-binding-modal", () => ({
  InstallSchedulerBindingModal: () => null,
}));

vi.mock("@/components/project/scheduler-bindings/uninstall-binding-modal", () => ({
  UninstallSchedulerBindingModal: () => null,
}));

vi.mock("@/hooks/store/user", () => ({
  useUserPermissions: () => ({ allowPermissions: () => true }),
}));

import { SchedulerBindingsPanel } from "../../core/components/project/scheduler-bindings/bindings-panel";

function makeBinding(): ISchedulerBinding {
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
    created_at: "2026-09-01T00:00:00Z",
    updated_at: "2026-09-01T00:00:00Z",
  };
}

function renderPanel() {
  useSWR.mockImplementation((key: unknown) => {
    const kind = Array.isArray(key) ? key[0] : null;
    if (kind === "scheduler-bindings") return { data: [makeBinding()], mutate: vi.fn() };
    if (kind === "schedulers") return { data: [], mutate: vi.fn() };
    return { data: undefined, mutate: vi.fn() };
  });
  return render(<SchedulerBindingsPanel workspaceSlug="acme" projectId="proj-1" />);
}

describe("SchedulerBindingsPanel row navigation", () => {
  beforeEach(() => {
    useSWR.mockReset();
    navigate.mockReset();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("navigates to the binding detail page when the row is clicked", () => {
    renderPanel();

    fireEvent.click(screen.getByText("Nightly Audit"));
    expect(navigate).toHaveBeenCalledWith("/acme/projects/proj-1/schedulers/binding-1");
  });

  it("does not navigate when the Edit / Uninstall actions or the toggle are clicked", () => {
    renderPanel();

    fireEvent.click(screen.getByText("Edit"));
    fireEvent.click(screen.getByText("Uninstall"));
    fireEvent.click(screen.getByRole("switch"));
    expect(navigate).not.toHaveBeenCalled();
  });
});
