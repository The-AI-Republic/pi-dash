// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Run detail pane (NEWFRONT-545, RUN-017–021). Pane states, executor and
// scheduler rendering, diagnostics with and without structure, results,
// the cloud-only tool panel, the transcript and the cancel confirmation.
import type { AgentRun, RunErrorDiagnostic, RunEvent } from "@pidash/api-client";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  canCancelRun,
  cancelBodyKind,
  RUN_CANCEL_CLOUD_BODY,
  RUN_CANCEL_LOCAL_BODY,
  RUN_DETAIL_LOADING_TEXT,
  RUN_DETAIL_NONE_TEXT,
  RUN_DETAIL_UNAVAILABLE_TEXT,
  RunDetail,
} from "./RunDetail.js";

const run = (overrides: Partial<AgentRun> = {}): AgentRun =>
  ({
    id: "11111111-2222-3333-4444-555555555555",
    status: "running",
    executor_kind: "local_runner",
    prompt: "do the thing",
    error: "",
    error_diagnostic: null,
    done_payload: null,
    tool_plan: {},
    tool_calls: [],
    events: [],
    scheduler_binding: null,
    scheduler_binding_detail: null,
    ...overrides,
  }) as AgentRun;

const diagnostic = (overrides: Partial<RunErrorDiagnostic> = {}): RunErrorDiagnostic => ({
  source: "agent",
  source_label: "Agent CLI",
  kind: "agent_authentication",
  summary: "auth failed",
  action: "Re-authenticate the agent",
  ...overrides,
});

const event = (overrides: Partial<RunEvent> = {}): RunEvent =>
  ({ id: 1, seq: 1, kind: "run/started", payload: {}, created_at: "2024-01-01T00:00:00Z", ...overrides }) as RunEvent;

function renderDetail(overrides: Partial<React.ComponentProps<typeof RunDetail>> = {}) {
  const onCancel = vi.fn();
  render(
    <RunDetail
      state="loaded"
      run={run()}
      cancelling={false}
      onCancel={onCancel}
      schedulerHref={() => null}
      {...overrides}
    />
  );
  return { onCancel };
}

describe("pane states", () => {
  it("renders none, unavailable and loading without a run", () => {
    const { unmount } = render(
      <RunDetail state="none" run={null} cancelling={false} onCancel={vi.fn()} schedulerHref={() => null} />
    );
    expect(screen.getByText(RUN_DETAIL_NONE_TEXT)).toBeInTheDocument();
    unmount();
    render(
      <RunDetail state="unavailable" run={null} cancelling={false} onCancel={vi.fn()} schedulerHref={() => null} />
    );
    expect(screen.getByText(RUN_DETAIL_UNAVAILABLE_TEXT)).toBeInTheDocument();
    render(<RunDetail state="loading" run={null} cancelling={false} onCancel={vi.fn()} schedulerHref={() => null} />);
    expect(screen.getByText(RUN_DETAIL_LOADING_TEXT)).toBeInTheDocument();
  });

  it("handles a loaded state with no run defensively", () => {
    render(<RunDetail state="loaded" run={null} cancelling={false} onCancel={vi.fn()} schedulerHref={() => null} />);
    expect(screen.getByText(RUN_DETAIL_LOADING_TEXT)).toBeInTheDocument();
  });
});

describe("identity", () => {
  it("shows the run id, status and executor badge", () => {
    renderDetail();
    expect(screen.getByTestId("run-detail-id")).toHaveTextContent("11111111-2222-3333-4444-555555555555");
    expect(screen.getByTestId("run-status")).toHaveTextContent("running");
    expect(screen.getByTestId("run-executor")).toHaveTextContent("Local Runner");
    expect(screen.getByTestId("run-prompt")).toHaveTextContent("do the thing");
  });

  it("labels a cloud run and hides the scheduler row without a link", () => {
    renderDetail({
      run: run({ executor_kind: "cloud_agent" }),
      schedulerHref: () => null,
    });
    expect(screen.getByTestId("run-executor")).toHaveTextContent("Pi Dash Cloud Agent");
    expect(screen.queryByTestId("run-scheduler-link")).toBeNull();
  });

  it("links the scheduler when a href resolves", () => {
    renderDetail({
      run: run({
        scheduler_binding: "b-1",
        scheduler_binding_detail: { id: "b-1", scheduler_name: "Nightly", scheduler_slug: "nightly", project: "p-1" },
      }),
      schedulerHref: () => "/acme/projects/p-1/schedulers/b-1",
    });
    expect(screen.getByTestId("run-scheduler-link")).toHaveAttribute("href", "/acme/projects/p-1/schedulers/b-1");
    expect(screen.getByTestId("run-scheduler-link")).toHaveTextContent("Nightly");
  });

  it("links the scheduler without a detail name", () => {
    renderDetail({ schedulerHref: () => "/acme/projects/p-1/schedulers/b-1" });
    expect(screen.getByTestId("run-scheduler-link")).toHaveAttribute("href", "/acme/projects/p-1/schedulers/b-1");
    expect(screen.getByTestId("run-scheduler-link")).toHaveTextContent("");
  });
});

describe("failure and result", () => {
  it("shows only the raw error when no diagnostic exists", () => {
    renderDetail({ run: run({ status: "failed", error: "boom" }) });
    expect(screen.getByTestId("run-error")).toHaveTextContent("boom");
    expect(screen.queryByTestId("run-error-diagnostic")).toBeNull();
  });

  it("shows the structured diagnostic above the raw error", () => {
    renderDetail({ run: run({ status: "failed", error: "boom", error_diagnostic: diagnostic() }) });
    expect(screen.getByTestId("run-error-source")).toHaveTextContent("Agent CLI");
    expect(screen.getByTestId("run-error-kind")).toHaveTextContent("Authentication");
    expect(screen.getByTestId("run-error-summary")).toHaveTextContent("auth failed");
    expect(screen.getByTestId("run-error-action")).toHaveTextContent("Re-authenticate the agent");
    expect(screen.getByTestId("run-error")).toHaveTextContent("boom");
  });

  it("falls back to the raw kind and omits an empty action", () => {
    renderDetail({
      run: run({
        status: "failed",
        error: "boom",
        error_diagnostic: diagnostic({ kind: "future_kind", action: "", source: "unknown", source_label: "Unknown" }),
      }),
    });
    expect(screen.getByTestId("run-error-kind")).toHaveTextContent("future_kind");
    expect(screen.queryByTestId("run-error-action")).toBeNull();
  });

  it("renders a result payload and a cloud summary above it", () => {
    renderDetail({
      run: run({ executor_kind: "cloud_agent", done_payload: { summary: "all done", answer: 42 } }),
    });
    expect(screen.getByTestId("run-result-summary")).toHaveTextContent("all done");
    expect(JSON.parse(screen.getByTestId("run-result").textContent ?? "{}")).toEqual({
      summary: "all done",
      answer: 42,
    });
  });

  it("renders a local result without a summary", () => {
    renderDetail({ run: run({ done_payload: { answer: 7 } }) });
    expect(screen.queryByTestId("run-result-summary")).toBeNull();
    expect(JSON.parse(screen.getByTestId("run-result").textContent ?? "{}")).toEqual({ answer: 7 });
  });

  it("hides result and error sections when they are absent", () => {
    renderDetail();
    expect(screen.queryByTestId("run-result")).toBeNull();
    expect(screen.queryByTestId("run-error")).toBeNull();
  });
});

describe("cloud execution panel", () => {
  it("lists planned tools and executed calls for cloud runs", () => {
    renderDetail({
      run: run({
        executor_kind: "cloud_agent",
        tool_plan: { tools: ["search", "fetch"] },
        tool_calls: [
          {
            id: "c-1",
            tool_call_id: "t-1",
            source: "s",
            server_key: "k",
            tool_name: "search",
            risk: "read",
            status: "succeeded",
            error_code: "",
            prepared_at: "2024-01-01T00:00:00Z",
            submitted_at: null,
            completed_at: null,
          },
        ],
      }),
    });
    expect(screen.getByTestId("run-cloud-panel")).toBeInTheDocument();
    expect(screen.getAllByTestId("run-cloud-tool").map((node) => node.textContent)).toEqual(["search", "fetch"]);
    expect(screen.getByTestId("run-cloud-call-name")).toHaveTextContent("search");
    expect(screen.getByTestId("run-cloud-call-meta")).toHaveTextContent("read · succeeded");
  });

  it("renders without calls and never for local runs", () => {
    const { unmount } = render(
      <RunDetail
        state="loaded"
        run={run({ executor_kind: "cloud_agent" })}
        cancelling={false}
        onCancel={vi.fn()}
        schedulerHref={() => null}
      />
    );
    expect(screen.queryByTestId("run-cloud-calls")).toBeNull();
    unmount();
    render(
      <RunDetail
        state="loaded"
        run={run({ tool_plan: { tools: ["ignored"] } })}
        cancelling={false}
        onCancel={vi.fn()}
        schedulerHref={() => null}
      />
    );
    expect(screen.queryByTestId("run-cloud-panel")).toBeNull();
  });
});

describe("events", () => {
  it("renders sequenced rows with inline narratives", () => {
    renderDetail({
      run: run({
        events: [
          event({ id: 1, seq: 1, kind: "run/started" }),
          event({ id: 2, seq: 2, kind: "agent/message", payload: { text: "hello" } }),
        ],
      }),
    });
    expect(screen.getByText("Events (2)")).toBeInTheDocument();
    const rows = screen.getAllByTestId("run-event-row");
    expect(rows).toHaveLength(2);
    expect(within(rows[1]!).getByText("agent/message")).toBeInTheDocument();
    expect(screen.getAllByTestId("run-event-narrative")).toHaveLength(1);
    expect(screen.getByTestId("run-event-narrative")).toHaveTextContent("hello");
  });

  it("keeps metadata events bare and shows an empty transcript", () => {
    const { unmount } = render(
      <RunDetail
        state="loaded"
        run={run({ events: [event()] })}
        cancelling={false}
        onCancel={vi.fn()}
        schedulerHref={() => null}
      />
    );
    expect(screen.getAllByTestId("run-event-row")).toHaveLength(1);
    expect(screen.queryByTestId("run-event-narrative")).toBeNull();
    unmount();
    render(<RunDetail state="loaded" run={run()} cancelling={false} onCancel={vi.fn()} schedulerHref={() => null} />);
    expect(screen.getByText("Events (0)")).toBeInTheDocument();
  });

  it("handles a run whose events field is absent", () => {
    renderDetail({ run: run({ events: undefined as unknown as RunEvent[] }) });
    expect(screen.getByText("Events (0)")).toBeInTheDocument();
    expect(screen.queryByTestId("run-event-row")).toBeNull();
  });
});

describe("cancellation", () => {
  it("offers cancel only for unsettled runs", () => {
    expect(canCancelRun({ status: "running" })).toBe(true);
    expect(canCancelRun({ status: "completed" })).toBe(false);
    expect(canCancelRun({ status: "cancel_requested" })).toBe(false);
    expect(canCancelRun({ status: "queued" })).toBe(true);
  });

  it("uses executor-specific consequence copy", () => {
    expect(cancelBodyKind("cloud_agent")).toBe(RUN_CANCEL_CLOUD_BODY);
    expect(cancelBodyKind("local_runner")).toBe(RUN_CANCEL_LOCAL_BODY);
  });

  it("confirms and cancels", async () => {
    const { onCancel } = renderDetail();
    await userEvent.click(screen.getByTestId("run-cancel"));
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByRole("heading", { name: "Cancel run?" })).toBeInTheDocument();
    expect(within(dialog).getByTestId("run-cancel-body")).toHaveTextContent(RUN_CANCEL_LOCAL_BODY);
    await userEvent.click(within(dialog).getByTestId("run-cancel-confirm"));
    expect(onCancel).toHaveBeenCalledWith(expect.objectContaining({ id: "11111111-2222-3333-4444-555555555555" }));
  });

  it("closes without cancelling", async () => {
    const { onCancel } = renderDetail();
    await userEvent.click(screen.getByTestId("run-cancel"));
    const dialog = screen.getByRole("dialog");
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(onCancel).not.toHaveBeenCalled();
  });

  it("closes through the dialog's own close control", async () => {
    renderDetail();
    await userEvent.click(screen.getByTestId("run-cancel"));
    const dialog = screen.getByRole("dialog");
    await userEvent.click(within(dialog).getByRole("button", { name: "Close dialog" }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("pins the dialog and ignores confirm/close while a cancel is in flight", async () => {
    const { onCancel } = renderDetail({ cancelling: true });
    await userEvent.click(screen.getByTestId("run-cancel"));
    const dialog = screen.getByRole("dialog");
    await userEvent.click(within(dialog).getByTestId("run-cancel-confirm"));
    expect(onCancel).not.toHaveBeenCalled();
    expect(within(dialog).queryByRole("button", { name: "Close dialog" })).toBeNull();
  });

  it("keeps the dialog open from its cancel control while submitting", async () => {
    renderDetail({ cancelling: true });
    await userEvent.click(screen.getByTestId("run-cancel"));
    const dialog = screen.getByRole("dialog");
    const cancel = within(dialog).getByRole("button", { name: "Cancel" });
    expect(cancel).toHaveAttribute("aria-disabled", "true");
    await userEvent.click(cancel);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("hides cancel for terminal runs", () => {
    renderDetail({ run: run({ status: "cancelled" }) });
    expect(screen.queryByTestId("run-cancel")).toBeNull();
  });
});
