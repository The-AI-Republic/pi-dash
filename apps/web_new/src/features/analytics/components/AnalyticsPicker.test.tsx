// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics picker tests (parity/specs/analytics.md §4, ANL-004).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

import { AnalyticsPicker } from "./AnalyticsPicker.js";

const WORKSPACE = "44444444-4444-4444-8444-444444444444";

function projectRow(id: string, name: string, identifier: string, overrides: Record<string, unknown> = {}) {
  return {
    id,
    name,
    identifier,
    sort_order: null,
    logo_props: {},
    member_role: 15,
    intake_count: 0,
    archived_at: null,
    workspace: WORKSPACE,
    cycle_view: false,
    issue_views_view: false,
    module_view: false,
    page_view: false,
    inbox_view: false,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 0,
    created_at: "2026-01-01T00:00:00.000Z",
    updated_at: "2026-01-01T00:00:00.000Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

const ALPHA = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const BETA = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const GAMMA = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const DELTA = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";

const ROWS = [
  projectRow(ALPHA, "Alpha", "ALP"),
  projectRow(BETA, "Beta", "BET"),
  projectRow(GAMMA, "Gamma", "GAM"),
  projectRow(DELTA, "Delta", "DEL"),
  projectRow("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee", "Retired", "RET", {
    archived_at: "2026-02-01T00:00:00.000Z",
  }),
];

const SELECTABLE = [
  { id: ALPHA, name: "Alpha" },
  { id: BETA, name: "Beta" },
  { id: GAMMA, name: "Gamma" },
  { id: DELTA, name: "Delta" },
];

function harness() {
  const transport = async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => '{"csrf_token":"csrf"}',
      };
    }
    return { status: 200, url, headers: { get: () => "application/json" }, text: async () => JSON.stringify(ROWS) };
  };
  createAppClient("https://api.test", transport as never);
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

function renderPicker(value: readonly string[], onChange: (ids: string[]) => void = () => {}): void {
  render(
    <QueryClientProvider client={harness()}>
      <AnalyticsPicker workspaceSlug="acme" projects={SELECTABLE} value={value} onChange={onChange} />
    </QueryClientProvider>
  );
}

function optionNames(): string[] {
  return within(screen.getByRole("listbox"))
    .getAllByRole("option")
    .map((row) => row.querySelector(":scope > span.truncate")?.textContent ?? "");
}

afterEach(() => resetAppClient());

describe("AnalyticsPicker", () => {
  it("labels the trigger by selection size", () => {
    const { rerender, unmount } = render(
      <QueryClientProvider client={harness()}>
        <AnalyticsPicker workspaceSlug="acme" projects={SELECTABLE} value={[]} onChange={() => {}} />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("analytics-picker-trigger")).toHaveTextContent("All projects");
    rerender(
      <QueryClientProvider client={harness()}>
        <AnalyticsPicker workspaceSlug="acme" projects={SELECTABLE} value={[BETA]} onChange={() => {}} />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("analytics-picker-trigger")).toHaveTextContent("Beta");
    rerender(
      <QueryClientProvider client={harness()}>
        <AnalyticsPicker workspaceSlug="acme" projects={SELECTABLE} value={[ALPHA, BETA, GAMMA]} onChange={() => {}} />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("analytics-picker-trigger")).toHaveTextContent("Alpha, Beta, Gamma");
    rerender(
      <QueryClientProvider client={harness()}>
        <AnalyticsPicker
          workspaceSlug="acme"
          projects={SELECTABLE}
          value={[ALPHA, BETA, GAMMA, DELTA]}
          onChange={() => {}}
        />
      </QueryClientProvider>
    );
    expect(screen.getByTestId("analytics-picker-trigger")).toHaveTextContent("3+ projects");
    unmount();
  });

  it("drops unresolvable ids from the summary", () => {
    renderPicker([ALPHA, "ffffffff-ffff-4fff-8fff-ffffffffffff"]);
    expect(screen.getByTestId("analytics-picker-trigger")).toHaveTextContent("Alpha");
  });

  it("offers only the selectable projects and toggles through onChange", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderPicker([], onChange);
    await user.click(screen.getByTestId("analytics-picker-trigger"));
    await waitFor(() => expect(screen.getByRole("listbox")).toBeInTheDocument());
    expect(optionNames()).toEqual(["Alpha", "Beta", "Gamma", "Delta"]);
    await user.click(within(screen.getByRole("listbox")).getByRole("option", { name: "Beta" }));
    expect(onChange).toHaveBeenCalledWith([BETA]);
  });

  it("searches names and identifiers", async () => {
    const user = userEvent.setup();
    renderPicker([]);
    await user.click(screen.getByTestId("analytics-picker-trigger"));
    await waitFor(() => expect(screen.getByRole("listbox")).toBeInTheDocument());
    await user.type(screen.getByRole("combobox", { name: "Search projects" }), "gam");
    await waitFor(() => expect(optionNames()).toEqual(["Gamma"]));
    await user.clear(screen.getByRole("combobox", { name: "Search projects" }));
    await user.type(screen.getByRole("combobox", { name: "Search projects" }), "BET");
    await waitFor(() => expect(optionNames()).toEqual(["Beta"]));
  });
});
