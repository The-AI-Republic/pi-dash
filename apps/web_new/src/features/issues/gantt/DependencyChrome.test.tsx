// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline dependency chrome (ISS-237): the community build never mounts
// it, a cloud build reads the drawn rows' finish-to-start relations and
// paints one arrow per edge, and a drag from one bar's handle onto another
// row writes the edge from the waiting item's side.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { edition } from "../../../core/edition/index.js";
import { DependencyChrome, showsGanttDependencies } from "./DependencyChrome.js";
import { DependencyLayer, laneAt, layerPoint } from "./DependencyLayer.js";
import { dependencyEnds } from "./dependencies.js";
import { GANTT_DEPENDENCIES_FLAG } from "../edition/flags.js";
import { ROW_HEIGHT, type GanttRow } from "./rows.js";
import type { GanttRange } from "./scale.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const A = "a23e4567-e89b-12d3-a456-426614174001";
const B = "b23e4567-e89b-12d3-a456-426614174002";
const RANGE: GanttRange = { startIso: "2026-10-01", dayCount: 30 };

function row(id: string, overrides: Partial<GanttRow> = {}): GanttRow {
  return {
    id,
    identifier: "WEB-1",
    name: "Row",
    startIso: "2026-10-01",
    targetIso: "2026-10-03",
    durationDays: 3,
    stateId: null,
    sortOrder: 1,
    priority: "none",
    sequenceId: 1,
    archived: false,
    unconfirmed: false,
    isEpic: false,
    ...overrides,
  };
}

const calls: { method: string; url: string; body: unknown }[] = [];
let finishAfter: Record<string, string[]> = {};

function related(id: string) {
  return {
    id,
    name: "Row",
    state_id: null,
    sort_order: 1,
    priority: "none",
    sequence_id: 1,
    project_id: PROJECT,
    label_ids: [],
    assignee_ids: [],
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    relation_type: "finish_after",
  };
}

function serve(): void {
  createAppClient("https://api.test", (async (url: string, init?: RequestInit) => {
    const method = init?.method ?? "GET";
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify({ csrf_token: "c" }) };
    }
    calls.push({ method, url, body: init?.body === undefined ? undefined : JSON.parse(String(init.body)) });
    const match = /issues\/([^/]+)\/issue-relation\/$/.exec(url);
    const issueId = match?.[1] ?? "";
    const body =
      method === "GET"
        ? {
            blocking: [],
            blocked_by: [],
            duplicate: [],
            relates_to: [],
            start_after: [],
            start_before: [],
            finish_after: (finishAfter[issueId] ?? []).map(related),
            finish_before: [],
          }
        : [];
    return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
  }) as never);
}

function renderChrome(rows: GanttRow[] = [row(A), row(B, { startIso: "2026-10-06" })]) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <DependencyChrome workspaceSlug={SLUG} projectId={PROJECT} rows={rows} range={RANGE} zoom="week" width={480} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  calls.length = 0;
  finishAfter = {};
  serve();
});
afterEach(() => {
  cleanup();
  resetAppClient();
  delete edition.flags[GANTT_DEPENDENCIES_FLAG];
});

describe("showsGanttDependencies", () => {
  it("needs both the flag and a project", () => {
    expect(showsGanttDependencies(PROJECT, true)).toBe(true);
    expect(showsGanttDependencies(PROJECT, false)).toBe(false);
    expect(showsGanttDependencies(undefined, true)).toBe(false);
    expect(showsGanttDependencies("", true)).toBe(false);
  });
});

describe("DependencyChrome", () => {
  it("reads nothing and paints nothing in the community build", async () => {
    finishAfter = { [A]: [B] };
    renderChrome();
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(screen.queryByTestId("gantt-dependency-layer")).toBeNull();
    expect(screen.queryAllByTestId("gantt-dependency-handle")).toHaveLength(0);
    expect(screen.queryAllByTestId("gantt-dependency-arrow")).toHaveLength(0);
    expect(calls).toHaveLength(0);
  });

  it("draws one arrow per finish-to-start edge, read from each drawn row", async () => {
    edition.flags[GANTT_DEPENDENCIES_FLAG] = true;
    finishAfter = { [A]: [B] };
    renderChrome();
    await waitFor(() => expect(screen.getAllByTestId("gantt-dependency-arrow")).toHaveLength(1));
    expect(calls.filter((call) => call.method === "GET").map((call) => call.url)).toEqual(
      expect.arrayContaining([
        expect.stringContaining(`/issues/${A}/issue-relation/`),
        expect.stringContaining(`/issues/${B}/issue-relation/`),
      ])
    );
    expect(screen.getByTestId("gantt-dependency-layer")).toHaveAttribute("width", "480");
  });

  it("skips a read for an unconfirmed create", async () => {
    edition.flags[GANTT_DEPENDENCIES_FLAG] = true;
    renderChrome([row(A), row("temp-1", { unconfirmed: true })]);
    await waitFor(() => expect(calls.some((call) => call.url.includes(A))).toBe(true));
    expect(calls.some((call) => call.url.includes("temp-1"))).toBe(false);
  });

  it("links on a handle drag onto another row's lane", async () => {
    edition.flags[GANTT_DEPENDENCIES_FLAG] = true;
    renderChrome();
    const handle = (await screen.findAllByTestId("gantt-dependency-handle"))[0] as Element;
    fireEvent.mouseDown(handle, { clientX: 120, clientY: 22 });
    expect(screen.getByTestId("gantt-dependency-drag-line")).toBeInTheDocument();
    const lane = document.createElement("div");
    lane.setAttribute("data-gantt-lane", "true");
    lane.setAttribute("data-lane-id", B);
    document.body.append(lane);
    fireEvent.mouseUp(lane);
    lane.remove();
    await waitFor(() => expect(calls.some((call) => call.method === "POST")).toBe(true));
    const post = calls.find((call) => call.method === "POST");
    expect(post?.url).toContain(`/issues/${B}/issue-relation/`);
    expect(post?.body).toEqual({ relation_type: "finish_after", issues: [A] });
  });

  it("drops a link released on its own row, an unknown row or after Escape", async () => {
    edition.flags[GANTT_DEPENDENCIES_FLAG] = true;
    renderChrome();
    const handle = (await screen.findAllByTestId("gantt-dependency-handle"))[0] as Element;
    fireEvent.mouseDown(handle, { clientX: 120, clientY: 22 });
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByTestId("gantt-dependency-drag-line")).toBeNull();
    fireEvent.mouseDown(handle, { clientX: 120, clientY: 22 });
    const own = document.createElement("div");
    own.setAttribute("data-gantt-lane", "true");
    own.setAttribute("data-lane-id", A);
    document.body.append(own);
    fireEvent.mouseUp(own);
    own.remove();
    fireEvent.mouseDown(handle, { clientX: 120, clientY: 22 });
    fireEvent.mouseUp(document.body);
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(calls.some((call) => call.method === "POST")).toBe(false);
  });
});

describe("drag geometry", () => {
  it("converts a viewport point into layer coordinates", () => {
    expect(layerPoint({ left: 40, top: 12 }, 100, 50)).toEqual({ x: 60, y: 38 });
  });

  it("reads the lane a release landed in", () => {
    const lane = document.createElement("div");
    lane.setAttribute("data-gantt-lane", "true");
    lane.setAttribute("data-lane-id", B);
    const child = document.createElement("span");
    lane.append(child);
    expect(laneAt(child)).toBe(B);
    expect(laneAt(document.body)).toBeNull();
    expect(laneAt(null)).toBeNull();
  });
});

describe("DependencyLayer", () => {
  it("tracks the pointer while a drag is live", async () => {
    const onLink = vi.fn();
    const ends = dependencyEnds({ rows: [row(A), row(B)], range: RANGE, zoom: "week", rowHeight: ROW_HEIGHT });
    render(<DependencyLayer ends={ends} edges={[]} width={480} height={ROW_HEIGHT * 2} onLink={onLink} />);
    fireEvent.mouseDown(screen.getAllByTestId("gantt-dependency-handle")[0]!, { clientX: 10, clientY: 20 });
    fireEvent.mouseMove(window, { clientX: 90, clientY: 60 });
    const line = screen.getByTestId("gantt-dependency-drag-line");
    expect(line).toHaveAttribute("x2", "90");
    expect(line).toHaveAttribute("y2", "60");
    const lane = document.createElement("div");
    lane.setAttribute("data-gantt-lane", "true");
    lane.setAttribute("data-lane-id", B);
    document.body.append(lane);
    fireEvent.mouseUp(lane);
    lane.remove();
    expect(onLink).toHaveBeenCalledWith(A, B);
  });

  it("refuses a lane the caller will not link to", () => {
    const onLink = vi.fn();
    const ends = dependencyEnds({ rows: [row(A), row(B)], range: RANGE, zoom: "week", rowHeight: ROW_HEIGHT });
    render(
      <DependencyLayer
        ends={ends}
        edges={[]}
        width={480}
        height={ROW_HEIGHT * 2}
        onLink={onLink}
        canLinkTo={(id) => id !== B}
      />
    );
    fireEvent.mouseDown(screen.getAllByTestId("gantt-dependency-handle")[0]!, { clientX: 10, clientY: 20 });
    const lane = document.createElement("div");
    lane.setAttribute("data-gantt-lane", "true");
    lane.setAttribute("data-lane-id", B);
    document.body.append(lane);
    fireEvent.mouseUp(lane);
    lane.remove();
    expect(onLink).not.toHaveBeenCalled();
  });

  it("links every drawn lane when the caller gives no filter", () => {
    const onLink = vi.fn();
    const ends = dependencyEnds({ rows: [row(A), row(B)], range: RANGE, zoom: "week", rowHeight: ROW_HEIGHT });
    render(<DependencyLayer ends={ends} edges={[]} width={480} height={ROW_HEIGHT * 2} onLink={onLink} />);
    fireEvent.mouseDown(screen.getAllByTestId("gantt-dependency-handle")[0]!, { clientX: 10, clientY: 20 });
    const lane = document.createElement("div");
    lane.setAttribute("data-gantt-lane", "true");
    lane.setAttribute("data-lane-id", B);
    document.body.append(lane);
    fireEvent.mouseUp(lane);
    lane.remove();
    expect(onLink).toHaveBeenCalledWith(A, B);
  });

  it("renders nothing without a drawn bar", () => {
    render(<DependencyLayer ends={new Map()} edges={[]} width={200} height={100} />);
    expect(screen.queryByTestId("gantt-dependency-layer")).toBeNull();
  });

  it("draws arrows at the bars' own geometry without handles when linking is off", () => {
    const ends = dependencyEnds({
      rows: [row(A), row(B, { startIso: "2026-10-06" })],
      range: RANGE,
      zoom: "week",
      rowHeight: ROW_HEIGHT,
    });
    render(
      <DependencyLayer ends={ends} edges={[{ fromId: A, toId: B }]} width={480} height={ROW_HEIGHT * 2} top={48} />
    );
    expect(screen.getByTestId("gantt-dependency-layer")).toHaveStyle({ top: "48px" });
    expect(screen.getAllByTestId("gantt-dependency-arrow")).toHaveLength(1);
    expect(screen.queryAllByTestId("gantt-dependency-handle")).toHaveLength(0);
  });

  it("drops an edge whose ends the layer was not given", () => {
    const ends = dependencyEnds({ rows: [row(A)], range: RANGE, zoom: "week", rowHeight: ROW_HEIGHT });
    render(<DependencyLayer ends={ends} edges={[{ fromId: A, toId: "gone" }]} width={480} height={ROW_HEIGHT} />);
    expect(screen.queryAllByTestId("gantt-dependency-arrow")).toHaveLength(0);
  });
});
