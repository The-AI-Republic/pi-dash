// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Filter values: one set per project, toggled per value, cleared per
// dimension or whole, and the list hook resolving the predicate in the
// project timezone.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { CYCLES_PROJECT_ID, jsonResponse, makeCycleRow, makeProjectRow } from "../testing.js";
import { EMPTY_CYCLE_FILTERS } from "../lib/cycleFilters.js";
import { resetCycleFilterStore, useCycleFilterStore, useCycleListFilter } from "./filterStore.js";

const PROJECT_PATH = `/api/workspaces/acme/projects/${CYCLES_PROJECT_ID}/`;

let projectTimezone = "UTC";

beforeEach(() => {
  projectTimezone = "UTC";
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(PROJECT_PATH)) return jsonResponse(url, 200, makeProjectRow({ timezone: projectTimezone }));
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  // Unmount first: resetting the stores while the probe stays subscribed
  // re-renders it after the app client is gone and throws.
  cleanup();
  resetAppClient();
  resetCycleFilterStore();
});

function values(): string[] {
  const filters = useCycleFilterStore.getState().filtersFor(CYCLES_PROJECT_ID);
  return [...filters.status, ...filters.startDate, ...filters.endDate];
}

describe("cycle filter store", () => {
  it("starts empty for every project", () => {
    expect(useCycleFilterStore.getState().filtersFor(CYCLES_PROJECT_ID)).toEqual(EMPTY_CYCLE_FILTERS);
  });

  it("toggles lifecycle states per project", () => {
    useCycleFilterStore.getState().toggleStatus(CYCLES_PROJECT_ID, "upcoming");
    useCycleFilterStore.getState().toggleStatus("other-project", "draft");
    expect(useCycleFilterStore.getState().filtersFor(CYCLES_PROJECT_ID).status).toEqual(["upcoming"]);
    expect(useCycleFilterStore.getState().filtersFor("other-project").status).toEqual(["draft"]);
    useCycleFilterStore.getState().toggleStatus(CYCLES_PROJECT_ID, "upcoming");
    expect(useCycleFilterStore.getState().filtersFor(CYCLES_PROJECT_ID).status).toEqual([]);
    expect(useCycleFilterStore.getState().filtersFor("other-project").status).toEqual(["draft"]);
  });

  it("replaces one date window and leaves the other alone", () => {
    useCycleFilterStore.getState().setDateValues(CYCLES_PROJECT_ID, "startDate", ["1_weeks;after;fromnow"]);
    useCycleFilterStore
      .getState()
      .setDateValues(CYCLES_PROJECT_ID, "startDate", ["2_weeks;after;fromnow", "2026-11-01;after"]);
    useCycleFilterStore.getState().setDateValues(CYCLES_PROJECT_ID, "endDate", ["1_months;after;fromnow"]);
    const filters = useCycleFilterStore.getState().filtersFor(CYCLES_PROJECT_ID);
    expect(filters.startDate).toEqual(["2_weeks;after;fromnow", "2026-11-01;after"]);
    expect(filters.endDate).toEqual(["1_months;after;fromnow"]);
    expect(filters.status).toEqual([]);
  });

  it("removes one value and keeps its siblings", () => {
    useCycleFilterStore.getState().toggleStatus(CYCLES_PROJECT_ID, "upcoming");
    useCycleFilterStore.getState().toggleStatus(CYCLES_PROJECT_ID, "draft");
    useCycleFilterStore.getState().removeValue(CYCLES_PROJECT_ID, "status", "upcoming");
    expect(useCycleFilterStore.getState().filtersFor(CYCLES_PROJECT_ID).status).toEqual(["draft"]);
    useCycleFilterStore
      .getState()
      .setDateValues(CYCLES_PROJECT_ID, "startDate", ["1_weeks;after;fromnow", "2026-11-01;after"]);
    useCycleFilterStore.getState().removeValue(CYCLES_PROJECT_ID, "startDate", "1_weeks;after;fromnow");
    expect(useCycleFilterStore.getState().filtersFor(CYCLES_PROJECT_ID).startDate).toEqual(["2026-11-01;after"]);
  });

  it("clears one dimension and keeps the others", () => {
    useCycleFilterStore.getState().toggleStatus(CYCLES_PROJECT_ID, "upcoming");
    useCycleFilterStore.getState().setDateValues(CYCLES_PROJECT_ID, "startDate", ["1_weeks;after;fromnow"]);
    useCycleFilterStore.getState().clearDimension(CYCLES_PROJECT_ID, "status");
    expect(useCycleFilterStore.getState().filtersFor(CYCLES_PROJECT_ID).status).toEqual([]);
    expect(useCycleFilterStore.getState().filtersFor(CYCLES_PROJECT_ID).startDate).toEqual(["1_weeks;after;fromnow"]);
    // Clearing an empty dimension is a no-op that keeps the state stable.
    const before = useCycleFilterStore.getState().filters;
    useCycleFilterStore.getState().clearDimension(CYCLES_PROJECT_ID, "status");
    expect(useCycleFilterStore.getState().filters).toBe(before);
  });

  it("clears every dimension together and forgets unknown projects silently", () => {
    useCycleFilterStore.getState().toggleStatus(CYCLES_PROJECT_ID, "upcoming");
    useCycleFilterStore.getState().setDateValues(CYCLES_PROJECT_ID, "startDate", ["1_weeks;after;fromnow"]);
    useCycleFilterStore.getState().toggleStatus("other-project", "draft");
    useCycleFilterStore.getState().clearAll(CYCLES_PROJECT_ID);
    expect(values()).toEqual([]);
    expect(useCycleFilterStore.getState().filtersFor("other-project").status).toEqual(["draft"]);
    const before = useCycleFilterStore.getState().filters;
    useCycleFilterStore.getState().clearAll("missing-project");
    expect(useCycleFilterStore.getState().filters).toBe(before);
  });

  it("resets every project at once", () => {
    useCycleFilterStore.getState().toggleStatus(CYCLES_PROJECT_ID, "upcoming");
    useCycleFilterStore.getState().toggleStatus("other-project", "draft");
    resetCycleFilterStore();
    expect(values()).toEqual([]);
    expect(useCycleFilterStore.getState().filtersFor("other-project").status).toEqual([]);
  });
});

function Probe({ starts }: { starts?: readonly string[] | undefined }): React.ReactElement {
  const { predicate, active, clearAll } = useCycleListFilter("acme", CYCLES_PROJECT_ID);
  const rows = (starts ?? ["2026-11-01T12:00:00Z", "2026-09-01T12:00:00Z"]).map((start, index) =>
    makeCycleRow({
      name: index === 0 ? "November push" : "Current cycle",
      status: index === 0 ? "upcoming" : "current",
      start_date: start,
    })
  );
  return (
    <div>
      <span data-testid="probe-active">{String(active)}</span>
      <span data-testid="probe-kept">
        {rows
          .filter(predicate)
          .map((row) => row.name)
          .join(",")}
      </span>
      <button type="button" onClick={clearAll}>
        clear
      </button>
    </div>
  );
}

function renderProbe(starts?: readonly string[] | undefined): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <Probe starts={starts} />
    </QueryClientProvider>
  );
}

describe("useCycleListFilter", () => {
  it("reports inactive with an open predicate before anything applies", async () => {
    renderProbe();
    expect(await screen.findByTestId("probe-active")).toHaveTextContent("false");
    expect(screen.getByTestId("probe-kept")).toHaveTextContent("November push,Current cycle");
  });

  it("narrows by the stored values and clears back to every row", async () => {
    useCycleFilterStore.getState().toggleStatus(CYCLES_PROJECT_ID, "upcoming");
    renderProbe();
    expect(await screen.findByTestId("probe-active")).toHaveTextContent("true");
    expect(screen.getByTestId("probe-kept")).toHaveTextContent("November push");
    fireEvent.click(screen.getByRole("button", { name: "clear" }));
    expect(screen.getByTestId("probe-active")).toHaveTextContent("false");
    expect(screen.getByTestId("probe-kept")).toHaveTextContent("November push,Current cycle");
  });

  it("resolves date windows in the project timezone", async () => {
    projectTimezone = "America/New_York";
    useCycleFilterStore.getState().setDateValues(CYCLES_PROJECT_ID, "startDate", ["2026-11-01;after"]);
    // 02:00 UTC on Nov 1 still reads Oct 31 in New York, below the bound.
    // The probe first renders on the UTC fallback, then re-resolves once
    // the project read lands, so wait for the settled empty set.
    renderProbe(["2026-11-01T02:00:00Z", "2026-09-01T12:00:00Z"]);
    await waitFor(() => expect(screen.getByTestId("probe-kept")).toHaveTextContent(""));
    expect(screen.getByTestId("probe-active")).toHaveTextContent("true");
  });
});
