// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline peek (parity/specs/issues.md §14, ISS-049/165). The panel is
// the shared shell panel: eyebrow with the short key, an editable title
// persisted through the issue PATCH, and a body hydrated from the detail
// query. A pending detail read paints the skeleton first.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { Issue } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { clearCommands } from "../../../shared/commands/index.js";
import { FULL_SCREEN_PORTAL_ID } from "../../../shared/shell/document.js";
import { issueKeys } from "../api/keys.js";
import { GanttPeekPanel, stateNameOf } from "./GanttPeekPanel.js";
import type { GanttRow } from "./rows.js";
import { ganttStatesKey } from "./useGanttData.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "323e4567-e89b-12d3-a456-426614174002";
const STATE = "423e4567-e89b-12d3-a456-426614174003";

const STATES = [
  {
    id: STATE,
    project_id: PROJECT,
    workspace_id: "523e4567-e89b-12d3-a456-426614174004",
    name: "In Progress",
    color: "#2563eb",
    group: "started" as const,
    default: false,
    description: "",
    sequence: 2,
  },
];

const ROW: GanttRow = {
  id: ISSUE,
  identifier: "WEB-7",
  name: "Ship it",
  startIso: "2026-10-01",
  targetIso: "2026-10-05",
  durationDays: 5,
  stateId: null,
  archived: false,
  unconfirmed: false,
};

function issue(overrides: Partial<Issue> = {}): Issue {
  return {
    id: ISSUE,
    name: "Ship it",
    state_id: null,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "none",
    sequence_id: 7,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "<p></p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
    ...overrides,
  };
}

interface Call {
  url: string;
  method: string;
  body: string;
}

function renderPanel(
  options: { detail?: Issue | null; hold?: boolean; seed?: boolean; states?: typeof STATES | false } = {}
) {
  document.body.innerHTML = `<div id="${FULL_SCREEN_PORTAL_ID}"></div>`;
  const calls: Call[] = [];
  let release: () => void = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    calls.push({ url, method, body: init.body ?? "" });
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    if (method === "PATCH") {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(issue({ name: JSON.parse(init.body ?? "{}").name })),
      };
    }
    if (options.hold === true) await held;
    if (options.detail === null) {
      return { status: 404, url, headers: { get: () => null }, text: async () => JSON.stringify({}) };
    }
    return {
      status: 200,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(options.detail ?? issue()),
    };
  }) as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (options.seed !== false) queryClient.setQueryData(issueKeys.detail(SLUG, ISSUE), options.detail ?? issue());
  if (options.states !== false) {
    queryClient.setQueryData(ganttStatesKey(SLUG, PROJECT), options.states ?? STATES);
  }
  const onClose = vi.fn();
  const view = render(
    <QueryClientProvider client={queryClient}>
      <GanttPeekPanel workspaceSlug={SLUG} projectId={PROJECT} row={ROW} onClose={onClose} />
    </QueryClientProvider>
  );
  return { calls, onClose, release, ...view };
}

afterEach(() => {
  resetAppClient();
  clearCommands();
});

describe("GanttPeekPanel", () => {
  it("renders the shared panel with the short key and title", async () => {
    renderPanel();
    const panel = screen.getByTestId("issue-preview");
    expect(document.getElementById(FULL_SCREEN_PORTAL_ID)).toContainElement(panel);
    expect(screen.getByText("WEB-7")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Work item title" })).toHaveValue("Ship it");
  });

  it("hydrates the body from the detail query", async () => {
    renderPanel({
      seed: false,
      detail: issue({
        state_id: STATE,
        start_date: "2026-10-01",
        target_date: "2026-10-05",
      } as Partial<Issue>),
    });
    expect(await screen.findByText("In Progress")).toBeInTheDocument();
    expect(screen.getByText("2026-10-01")).toBeInTheDocument();
    expect(screen.getByText("2026-10-05")).toBeInTheDocument();
  });

  it("shows the skeleton while the detail read is pending", async () => {
    const h = renderPanel({ hold: true, seed: false });
    expect(await screen.findByTestId("gantt-peek-skeleton")).toBeInTheDocument();
    h.release();
    await waitFor(() => expect(screen.queryByTestId("gantt-peek-skeleton")).not.toBeInTheDocument());
  });

  it("shows not-set dates and an unassigned state", async () => {
    renderPanel({ detail: issue() });
    expect(await screen.findByText("No state")).toBeInTheDocument();
    expect(screen.getAllByText("Not set")).toHaveLength(2);
  });

  it("falls back to no state when the id is not in the project's states", async () => {
    renderPanel({
      detail: issue({ state_id: "623e4567-e89b-12d3-a456-426614174005" } as Partial<Issue>),
    });
    expect(await screen.findByText("No state")).toBeInTheDocument();
  });

  it("falls back to no state until the state list arrives", async () => {
    renderPanel({
      detail: issue({ state_id: STATE } as Partial<Issue>),
      states: false,
    });
    expect(await screen.findByText("No state")).toBeInTheDocument();
  });

  it("persists a renamed title through the issue PATCH", async () => {
    const h = renderPanel();
    const field = screen.getByRole("textbox", { name: "Work item title" });
    fireEvent.change(field, { target: { value: "Ship it v2" } });
    fireEvent.keyDown(field, { key: "Enter" });
    await waitFor(() => expect(h.calls.some((call) => call.method === "PATCH")).toBe(true));
    const patch = h.calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toContain(`/issues/${ISSUE}/`);
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({ name: "Ship it v2" });
  });

  it("closes through the panel's close button and Escape", () => {
    const h = renderPanel();
    fireEvent.click(screen.getByRole("button", { name: "Close preview" }));
    expect(h.onClose).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(h.onClose).toHaveBeenCalledTimes(2);
  });
});

describe("stateNameOf", () => {
  it("names a known state", () => {
    expect(stateNameOf(STATES, STATE)).toBe("In Progress");
  });

  it("reads an unset state and an unknown id as no state", () => {
    expect(stateNameOf(STATES, null)).toBe("No state");
    expect(stateNameOf(STATES, "missing")).toBe("No state");
  });
});
