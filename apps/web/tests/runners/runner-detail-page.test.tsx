/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { IRunner } from "@pi-dash/types";

const { useSWR, getRunnerDetail } = vi.hoisted(() => ({
  useSWR: vi.fn(),
  getRunnerDetail: vi.fn(),
}));

vi.mock("swr", () => ({
  default: useSWR,
}));

vi.mock("react-router", () => ({
  useParams: () => ({ workspaceSlug: "acme", runnerId: "runner-1" }),
  useNavigate: () => vi.fn(),
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

vi.mock("@pi-dash/services", () => ({
  getRunnerDetail,
}));

vi.mock("@pi-dash/ui", () => ({
  Badge: ({ children }: { children: React.ReactNode }) => <span>{children}</span>,
  Button: ({ children }: { children: React.ReactNode }) => <button type="button">{children}</button>,
}));

vi.mock("@/components/core/page-title", () => ({
  PageHead: () => null,
}));

vi.mock("@/components/runners/runner-agent-status-panel", () => ({
  RunnerAgentStatusPanel: () => <div data-testid="agent-status-panel" />,
}));

import RunnerDetailPage from "../../app/(all)/[workspaceSlug]/runners/detail/[runnerId]/page";

const RUNNER: IRunner = {
  id: "runner-1",
  name: "browserx-local",
  status: "online",
  os: "linux",
  arch: "x86_64",
  runner_version: "0.1.12",
  dev_metadata: { working_dir: "/home/dev/projects/browserx" },
  protocol_version: 3,
  capabilities: ["codex", "claude"],
  last_heartbeat_at: "2026-05-23T00:00:00Z",
  owner: "user-1",
  pod: "pod-1",
  pod_detail: {
    id: "pod-1",
    name: "pod-a",
    is_default: false,
    project: "proj-1",
    project_identifier: "BROWSERXTE",
  },
  dev_machine_detail: { id: "dm-1", host_label: "mac-mini.local", label: "Rich's Mac mini" },
  connection: "conn-1",
  live_state: null,
  enrolled_at: "2026-05-22T00:00:00Z",
  revoked_at: null,
  revoked_reason: "",
  created_at: "2026-05-22T00:00:00Z",
  updated_at: "2026-05-23T00:00:00Z",
};

describe("RunnerDetailPage", () => {
  beforeEach(() => {
    useSWR.mockReset();
    getRunnerDetail.mockReset();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("renders the runner's metadata once loaded", () => {
    useSWR.mockReturnValue({ data: RUNNER, error: undefined, isLoading: false });
    render(<RunnerDetailPage />);

    expect(screen.getByText("browserx-local")).toBeTruthy();
    expect(screen.getByText("runner-1")).toBeTruthy();
    expect(screen.getByText("linux / x86_64")).toBeTruthy();
    expect(screen.getByText("0.1.12")).toBeTruthy();
    expect(screen.getByText("/home/dev/projects/browserx")).toBeTruthy();
    expect(screen.getByText("pod-a")).toBeTruthy();
    expect(screen.getByText("BROWSERXTE")).toBeTruthy();
    expect(screen.getByText("Rich's Mac mini")).toBeTruthy();
    expect(screen.getByText("codex")).toBeTruthy();
    expect(screen.getByText("claude")).toBeTruthy();
    expect(screen.getByTestId("agent-status-panel")).toBeTruthy();
  });

  it("shows a loading state while fetching", () => {
    useSWR.mockReturnValue({ data: undefined, error: undefined, isLoading: true });
    render(<RunnerDetailPage />);

    expect(screen.getByText("Loading…")).toBeTruthy();
  });

  it("shows an error state when the fetch fails", () => {
    useSWR.mockReturnValue({ data: undefined, error: new Error("boom"), isLoading: false });
    render(<RunnerDetailPage />);

    expect(screen.getByText("Failed to load runner")).toBeTruthy();
  });
});
