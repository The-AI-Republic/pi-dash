/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { IAgentRun, IAgentRunEvent, IAgentRunPage } from "@pi-dash/types";

const { useSWR, listRuns, getRun, cancelRun } = vi.hoisted(() => ({
  useSWR: vi.fn(),
  listRuns: vi.fn(),
  getRun: vi.fn(),
  cancelRun: vi.fn(),
}));

vi.mock("swr", () => ({
  default: useSWR,
}));

vi.mock("react-router", () => ({
  useParams: () => ({ workspaceSlug: "acme", runId: "run-1" }),
  useNavigate: () => vi.fn(),
  useSearchParams: () => [new URLSearchParams(), vi.fn()],
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

vi.mock("@pi-dash/propel/toast", () => ({
  TOAST_TYPE: { ERROR: "error" },
  setToast: vi.fn(),
}));

vi.mock("@pi-dash/services", () => ({
  RunnerService: class {
    listRuns = listRuns;
    getRun = getRun;
    cancelRun = cancelRun;
  },
}));

vi.mock("@pi-dash/ui", () => ({
  AlertModalCore: () => null,
  Badge: ({ children }: { children: React.ReactNode }) => <span>{children}</span>,
  Button: ({ children }: { children: React.ReactNode }) => <button type="button">{children}</button>,
  Spinner: () => <span>spinner</span>,
}));

vi.mock("@/components/core/page-title", () => ({
  PageHead: () => null,
}));

vi.mock("@/components/runners/runners-tabs", () => ({
  RunnersTabs: () => <div data-testid="runners-tabs" />,
}));

vi.mock("@/hooks/store/use-workspace", () => ({
  useWorkspace: () => ({ currentWorkspace: { id: "ws-1", name: "Acme" } }),
}));

import RunsPage from "../../app/(all)/[workspaceSlug]/runners/runs/page";

function makeEvent(overrides: Partial<IAgentRunEvent>): IAgentRunEvent {
  return {
    id: 1,
    seq: 1,
    kind: "runner/compact",
    payload: {},
    created_at: "2026-07-14T00:00:00Z",
    ...overrides,
  };
}

function makeRun(events: IAgentRunEvent[]): IAgentRun {
  return {
    id: "run-1",
    status: "completed",
    prompt: "change the button color",
    created_at: "2026-07-14T00:00:00Z",
    events,
  } as IAgentRun;
}

const EMPTY_PAGE: IAgentRunPage = {
  results: [],
  count: 0,
  total_count: 0,
  total_pages: 1,
} as IAgentRunPage;

/** Route the two useSWR call sites (runs list, run detail) by their key. */
function mockSwrWithDetail(detail: IAgentRun) {
  useSWR.mockImplementation((key: unknown) => {
    const kind = Array.isArray(key) ? key[0] : null;
    if (kind === "runner-runs") return { data: EMPTY_PAGE, mutate: vi.fn() };
    if (kind === "runner-run-detail") return { data: detail, error: undefined };
    return { data: undefined, error: undefined };
  });
}

describe("RunnerRunsPage agent narrative", () => {
  beforeEach(() => {
    useSWR.mockReset();
    listRuns.mockReset();
    getRun.mockReset();
    cancelRun.mockReset();
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("renders agent/message narrative text so the run reads like a CLI session", () => {
    const narrative = "Let me first explore the original color of the home page button.";
    mockSwrWithDetail(
      makeRun([
        makeEvent({ id: 1, seq: 1, kind: "runner/compact", payload: { summary: "3 low-signal agent events" } }),
        makeEvent({
          id: 2,
          seq: 2,
          kind: "agent/message",
          payload: { schema: "runner_agent_message_v1", text: narrative },
        }),
      ])
    );

    render(<RunsPage />);

    expect(screen.getByText("agent/message")).toBeTruthy();
    expect(screen.getByText(narrative)).toBeTruthy();
    // The compacted low-signal event stays a metadata-only row: its kind is
    // visible but its payload summary is not rendered as narrative.
    expect(screen.getByText("runner/compact")).toBeTruthy();
    expect(screen.queryByText("3 low-signal agent events")).toBeNull();
  });

  it("renders no narrative block when an agent/message payload has no usable text", () => {
    mockSwrWithDetail(
      makeRun([
        makeEvent({ id: 1, seq: 1, kind: "agent/message", payload: { schema: "runner_agent_message_v1" } }),
        makeEvent({
          id: 2,
          seq: 2,
          kind: "agent/message",
          payload: { schema: "runner_agent_message_v1", text: "   " },
        }),
        makeEvent({
          id: 3,
          seq: 3,
          kind: "agent/message",
          payload: { schema: "runner_agent_message_v1", text: 42 },
        }),
      ])
    );

    const { container } = render(<RunsPage />);

    expect(screen.getAllByText("agent/message")).toHaveLength(3);
    expect(container.querySelector("tbody pre")).toBeNull();
  });
});
