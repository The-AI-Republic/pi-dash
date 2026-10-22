// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One cycle row: the name links the detail address, then the ring,
// the lifecycle marker, the date span, and the extras for the row's
// state; the eye opens the quick look.

import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { fireEvent, render, screen } from "@testing-library/react";
import type { CycleRow as CycleRowData } from "@pidash/api-client";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { UNKNOWN_CYCLE_MEMBER, type CycleMemberEntry } from "../lib/cycleMembers.js";
import { CYCLES_CYCLE_ID, CYCLES_PROJECT_ID, makeCycleRow } from "../testing.js";
import { CycleRow, type CycleRowProps } from "./CycleRow.js";

const CREATOR_ID = "123e4567-e89b-12d3-a456-426614174010";
const ASSIGNEE_ID = "123e4567-e89b-12d3-a456-426614174011";

function directory(): Map<string, CycleMemberEntry> {
  return new Map([
    [CREATOR_ID, { displayName: "Ada Lovelace", avatarUrl: "https://cdn.test/ada.png" }],
    [ASSIGNEE_ID, { displayName: "Grace Hopper", avatarUrl: null }],
  ]);
}

async function renderRow(
  props: Partial<CycleRowProps> & { cycle: CycleRowData; onPeek?: (cycleId: string) => void }
): Promise<ReturnType<typeof render>> {
  const onPeek = props.onPeek ?? (() => undefined);
  const rootRoute = createRootRoute({ component: () => <Outlet /> });
  const wsRoute = createRoute({ getParentRoute: () => rootRoute, path: "/$ws", component: () => <Outlet /> });
  const listRoute = createRoute({
    getParentRoute: () => wsRoute,
    path: "projects/$projectId/cycles",
    component: () => (
      <CycleRow
        workspaceSlug="acme"
        projectId={CYCLES_PROJECT_ID}
        projectTimeZone="UTC"
        directory={directory()}
        showFavoriteMarker
        onPeek={onPeek}
        {...props}
        cycle={props.cycle}
      />
    ),
  });
  const detailRoute = createRoute({
    getParentRoute: () => wsRoute,
    path: "projects/$projectId/cycles/$cycleId",
    component: () => <p>detail</p>,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([wsRoute.addChildren([listRoute, detailRoute])]),
    history: createMemoryHistory({ initialEntries: [`/acme/projects/${CYCLES_PROJECT_ID}/cycles`] }),
    defaultPreload: false,
  });
  const result = render(<RouterProvider router={router} />);
  await screen.findByTestId("cycle-row");
  return result;
}

function upcomingRow(overrides: Partial<CycleRowData> = {}): CycleRowData {
  return makeCycleRow({
    status: "upcoming",
    total_issues: 2,
    completed_issues: 0,
    cancelled_issues: 0,
    assignee_ids: [ASSIGNEE_ID],
    created_by: CREATOR_ID,
    ...overrides,
  });
}

describe("CycleRow", async () => {
  it("links the name to the canonical detail address", async () => {
    await renderRow({ cycle: upcomingRow({ name: "Cycle 7" }) });
    const link = screen.getByRole("link");
    expect(link).toHaveAttribute("href", `/acme/projects/${CYCLES_PROJECT_ID}/cycles/${CYCLES_CYCLE_ID}`);
    expect(link).toHaveTextContent("Cycle 7");
  });

  it("shows the ring, marker, dates, count, and creator plus assignees on upcoming rows", async () => {
    await renderRow({ cycle: upcomingRow() });
    expect(screen.getByRole("img", { name: "0% complete" })).toBeInTheDocument();
    expect(screen.getByText("Yet to start")).toBeInTheDocument();
    expect(screen.getByTestId("cycle-date-span")).toHaveTextContent("Sep 01 - Dec 31, 2026");
    expect(screen.getByText("2")).toBeInTheDocument();
    expect(screen.getByText("work items")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Ada Lovelace" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Grace Hopper" })).toBeInTheDocument();
    expect(screen.queryByLabelText("No assignees")).not.toBeInTheDocument();
  });

  it("shows the count on drafts with the members icon when nobody is assigned", async () => {
    await renderRow({
      cycle: upcomingRow({ status: "draft", total_issues: 0, assignee_ids: [], created_by: CREATOR_ID }),
    });
    expect(screen.getByText("Draft")).toBeInTheDocument();
    expect(screen.getByText("0")).toBeInTheDocument();
    expect(screen.getByLabelText("No assignees")).toBeInTheDocument();
  });

  it("shows the creator alone on the active row with no count", async () => {
    await renderRow({ cycle: upcomingRow({ status: "current" }) });
    expect(screen.getByText("In progress")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Ada Lovelace" })).toBeInTheDocument();
    expect(screen.queryByRole("img", { name: "Grace Hopper" })).not.toBeInTheDocument();
    expect(screen.queryByText("work items")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("No assignees")).not.toBeInTheDocument();
  });

  it("renders no creator avatar when the row carries none", async () => {
    await renderRow({ cycle: upcomingRow({ status: "current", created_by: null }) });
    expect(screen.queryByRole("img", { name: "Ada Lovelace" })).not.toBeInTheDocument();
    expect(screen.queryByRole("img", { name: UNKNOWN_CYCLE_MEMBER.displayName })).not.toBeInTheDocument();
  });

  it("renders assignees without a creator on upcoming rows", async () => {
    await renderRow({ cycle: upcomingRow({ created_by: undefined }) });
    expect(screen.queryByRole("img", { name: "Ada Lovelace" })).not.toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Grace Hopper" })).toBeInTheDocument();
  });

  it("falls back to the unknown member for ids missing from the directory", async () => {
    await renderRow({
      cycle: upcomingRow({
        created_by: "123e4567-e89b-12d3-a456-426614174099",
        assignee_ids: ["123e4567-e89b-12d3-a456-426614174098"],
      }),
    });
    expect(screen.getAllByRole("img", { name: "Unknown member" })).toHaveLength(2);
  });

  it("reads the frozen snapshot on completed rows with no count", async () => {
    await renderRow({
      cycle: upcomingRow({
        status: "completed",
        total_issues: 9,
        completed_issues: 8,
        cancelled_issues: 0,
        assignee_ids: [],
        progress_snapshot: { total_issues: 4, completed_issues: 4, cancelled_issues: 0 },
      }),
    });
    expect(screen.getByText("Completed")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "100% complete" })).toBeInTheDocument();
    expect(screen.queryByText("work items")).not.toBeInTheDocument();
  });

  it("marks favorites selected and plain rows unselected", async () => {
    const { unmount } = await renderRow({ cycle: upcomingRow({ is_favorite: true }) });
    expect(screen.getByTestId("cycle-favorite-marker")).toHaveAttribute("data-favorite", "true");
    expect(screen.getByRole("img", { name: "Marked as favorite" })).toBeInTheDocument();
    unmount();
    await renderRow({ cycle: upcomingRow({ is_favorite: false }) });
    expect(screen.getByTestId("cycle-favorite-marker")).toHaveAttribute("data-favorite", "false");
    expect(screen.getByRole("img", { name: "Not marked as favorite" })).toBeInTheDocument();
  });

  it("hides the star when the marker is not offered", async () => {
    await renderRow({ cycle: upcomingRow(), showFavoriteMarker: false });
    expect(screen.queryByTestId("cycle-favorite-marker")).not.toBeInTheDocument();
  });

  it("renders no date element for undated rows and the single date for one-sided ranges", async () => {
    const { unmount } = await renderRow({ cycle: upcomingRow({ start_date: null, end_date: null }) });
    expect(screen.queryByTestId("cycle-date-span")).not.toBeInTheDocument();
    unmount();
    await renderRow({ cycle: upcomingRow({ start_date: "2026-09-01T12:00:00Z", end_date: null }) });
    expect(screen.getByTestId("cycle-date-span")).toHaveTextContent("Sep 01, 2026");
  });

  it("opens the quick look from the eye", async () => {
    const onPeek = vi.fn();
    await renderRow({ cycle: upcomingRow({ name: "Cycle 7" }), onPeek });
    fireEvent.click(screen.getByRole("button", { name: "Quick look at Cycle 7" }));
    expect(onPeek).toHaveBeenCalledWith(CYCLES_CYCLE_ID);
  });

  it("defaults the count to zero when the row omits counters", async () => {
    const row = upcomingRow({ total_issues: undefined, completed_issues: undefined, cancelled_issues: undefined });
    await renderRow({ cycle: row });
    expect(screen.getByText("0")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "0% complete" })).toBeInTheDocument();
  });
});
