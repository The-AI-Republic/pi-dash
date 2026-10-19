// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { EstimatePicker } from "./EstimatePicker.js";

const PROJECT = "22222222-2222-4222-8222-222222222222";
const WORKSPACE = "33333333-3333-4333-8333-333333333333";

function system(overrides: Record<string, unknown> = {}) {
  return {
    id: "11111111-1111-4111-8111-111111111111",
    workspace: WORKSPACE,
    project: PROJECT,
    name: "Points",
    description: "",
    type: "points",
    last_used: true,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

function point(id: string, key: number, value: string) {
  return { id, key, value, description: "" };
}
const POINT_1 = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const POINT_3 = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

const POINTS_SYSTEM = [
  system({
    points: [point(POINT_1, 1, "1"), point(POINT_3, 2, "3")],
  }),
];

const TIME_SYSTEM = [
  system({
    name: "Time",
    type: "time",
    points: [point(POINT_1, 1, "30"), point(POINT_3, 2, "90")],
  }),
];

const INACTIVE_SYSTEM = [{ ...POINTS_SYSTEM[0], last_used: false }];

function harness(body: unknown, status = 200, hang = false) {
  const calls: string[] = [];
  const transport = async (url: string) => {
    calls.push(url);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => '{"csrf_token":"csrf"}',
      };
    }
    if (hang) await new Promise(() => {});
    return { status, url, headers: { get: () => "application/json" }, text: async () => JSON.stringify(body) };
  };
  createAppClient("https://api.test", transport as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return { queryClient, calls };
}

function optionNames(): string[] {
  return within(screen.getByRole("listbox"))
    .getAllByRole("option")
    .map((row) => row.querySelector(":scope > span.truncate")?.textContent ?? "");
}

function renderPicker(props: Partial<React.ComponentProps<typeof EstimatePicker>> = {}, queryClient?: QueryClient) {
  const client = queryClient ?? new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <EstimatePicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={() => {}} {...props} />
    </QueryClientProvider>
  );
  return client;
}

afterEach(() => {
  resetAppClient();
});

describe("EstimatePicker", () => {
  it("fetches lazily and lists No estimate plus the points", async () => {
    const { queryClient, calls } = harness(POINTS_SYSTEM);
    renderPicker({}, queryClient);
    expect(calls).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "None" }));
    expect(await screen.findByRole("option", { name: "No estimate" })).toBeInTheDocument();
    expect(optionNames()).toEqual(["No estimate", "1", "3"]);
    expect(calls.some((url) => url.endsWith("/estimates/"))).toBe(true);
    for (const name of ["No estimate", "1", "3"]) {
      expect(screen.getByRole("option", { name }).querySelector("svg")).not.toBeNull();
    }
  });

  it("renders TIME points as hours and minutes", async () => {
    const { queryClient } = harness(TIME_SYSTEM);
    renderPicker({}, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "None" }));
    expect(await screen.findByRole("option", { name: "30m" })).toBeInTheDocument();
    expect(optionNames()).toEqual(["No estimate", "30m", "1h 30m"]);
  });

  it("assigns and clears the point", async () => {
    const { queryClient } = harness(POINTS_SYSTEM);
    const onChange = vi.fn();
    renderPicker({ onChange }, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "None" }));
    fireEvent.click(await screen.findByRole("option", { name: "3" }));
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith(POINT_3);
    fireEvent.click(screen.getByRole("button", { name: "None" }));
    fireEvent.click(await screen.findByRole("option", { name: "No estimate" }));
    expect(onChange).toHaveBeenCalledTimes(2);
    expect(onChange).toHaveBeenCalledWith(null);
  });

  it("searches points and shows the empty message on a miss", async () => {
    const { queryClient } = harness(POINTS_SYSTEM);
    const user = userEvent.setup();
    renderPicker({ value: POINT_1 }, queryClient);
    await user.click(screen.getByRole("button", { name: "None" }));
    await screen.findByRole("option", { name: "3" });
    await user.type(screen.getByRole("combobox", { name: "Search estimates" }), "3");
    expect(optionNames()).toEqual(["3"]);
    await user.type(screen.getByRole("combobox", { name: "Search estimates" }), " no-such-point");
    expect(screen.queryByRole("option")).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("No matching results");
  });

  it("shows one disabled row without an active system", async () => {
    const { queryClient } = harness(INACTIVE_SYSTEM);
    renderPicker({}, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "None" }));
    const row = await screen.findByRole("option", { name: "No estimate" });
    expect(row).toBeDisabled();
    expect(optionNames()).toEqual(["No estimate"]);
  });

  it("names the current point on the trigger, else the placeholder", async () => {
    const { queryClient } = harness(POINTS_SYSTEM);
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <EstimatePicker workspaceSlug="acme" projectId={PROJECT} value={POINT_3} onChange={() => {}} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "None" }));
    await screen.findByRole("option", { name: "3" });
    expect(screen.getByRole("button", { name: "3" })).toBeInTheDocument();
    rerender(
      <QueryClientProvider client={queryClient}>
        <EstimatePicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value="99999999-9999-4999-8999-999999999999"
          onChange={() => {}}
          placeholder="Pick one"
        />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Pick one" })).toBeInTheDocument();
  });

  it("supports icon-only and custom triggers", () => {
    const { queryClient } = harness(POINTS_SYSTEM);
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <EstimatePicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={() => {}} iconOnly />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "None" }).querySelector("svg")).not.toBeNull();
    rerender(
      <QueryClientProvider client={queryClient}>
        <EstimatePicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value={null}
          onChange={() => {}}
          trigger={<button type="button">Custom estimate</button>}
        />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Custom estimate" })).toBeInTheDocument();
  });

  it("follows a controlled open state", async () => {
    const { queryClient } = harness(POINTS_SYSTEM);
    const onOpenChange = vi.fn();
    renderPicker({ open: true, onOpenChange }, queryClient);
    expect(await screen.findByRole("option", { name: "No estimate" })).toBeInTheDocument();
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("reports controlled closes without managing state", async () => {
    const { queryClient } = harness(POINTS_SYSTEM);
    const onOpenChange = vi.fn();
    const user = userEvent.setup();
    renderPicker({ open: true, onOpenChange }, queryClient);
    await screen.findByRole("option", { name: "No estimate" });
    await user.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("does not fetch or open while disabled", () => {
    const { queryClient, calls } = harness(POINTS_SYSTEM);
    renderPicker({ disabled: true }, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "None" }));
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(calls).toEqual([]);
  });

  it("shows the loading row while fetching", async () => {
    const { queryClient } = harness(POINTS_SYSTEM, 200, true);
    renderPicker({}, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "None" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Loading estimates");
  });

  it("shows the error row when the read fails", async () => {
    const { queryClient } = harness({}, 500);
    renderPicker({}, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "None" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't load estimates.");
  });
});
