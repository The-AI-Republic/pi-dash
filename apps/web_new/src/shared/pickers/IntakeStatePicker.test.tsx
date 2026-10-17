// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { IntakeStatePicker } from "./IntakeStatePicker.js";

const PROJECT = "22222222-2222-4222-8222-222222222222";
const WORKSPACE = "33333333-3333-4333-8333-333333333333";
const TRIAGE_ID = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

const TRIAGE = {
  id: TRIAGE_ID,
  project_id: PROJECT,
  workspace_id: WORKSPACE,
  name: "Triage",
  color: "#5e6ad2",
  group: "triage",
  default: false,
  description: "",
  sequence: 1,
};

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

function renderPicker(props: Partial<React.ComponentProps<typeof IntakeStatePicker>> = {}, queryClient?: QueryClient) {
  const client = queryClient ?? new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <IntakeStatePicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={() => {}} {...props} />
    </QueryClientProvider>
  );
  return client;
}

afterEach(() => {
  resetAppClient();
});

describe("IntakeStatePicker", () => {
  it("fetches lazily and lists states with no clear row", async () => {
    const { queryClient, calls } = harness(TRIAGE);
    const onChange = vi.fn();
    renderPicker({ onChange }, queryClient);
    expect(calls).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Select intake state" }));
    expect(await screen.findByRole("option", { name: "Triage" })).toBeInTheDocument();
    expect(optionNames()).toEqual(["Triage"]);
    expect(optionNames().some((name) => /clear|none/i.test(name))).toBe(false);
    expect(calls.some((url) => url.endsWith("/intake-state/"))).toBe(true);
    expect(screen.getByRole("option", { name: "Triage" }).querySelector("svg")).not.toBeNull();
    fireEvent.click(screen.getByRole("option", { name: "Triage" }));
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith(TRIAGE_ID);
  });

  it("searches states and shows the empty message on a miss", async () => {
    const { queryClient } = harness(TRIAGE);
    const user = userEvent.setup();
    renderPicker({}, queryClient);
    await user.click(screen.getByRole("button", { name: "Select intake state" }));
    await screen.findByRole("option", { name: "Triage" });
    await user.type(screen.getByRole("combobox", { name: "Search intake states" }), "Tri");
    expect(optionNames()).toEqual(["Triage"]);
    await user.type(screen.getByRole("combobox", { name: "Search intake states" }), "-no-such-state-");
    expect(screen.queryByRole("option")).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("No matching results");
  });

  it("falls back to the first state on the trigger", async () => {
    const { queryClient } = harness(TRIAGE);
    renderPicker({}, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Select intake state" }));
    await screen.findByRole("option", { name: "Triage" });
    expect(screen.getByRole("button", { name: "Triage" })).toBeInTheDocument();
  });

  it("names a set state, else the placeholder for unknown values", async () => {
    const { queryClient } = harness(TRIAGE);
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <IntakeStatePicker workspaceSlug="acme" projectId={PROJECT} value={TRIAGE_ID} onChange={() => {}} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select intake state" }));
    await screen.findByRole("option", { name: "Triage" });
    expect(screen.getByRole("button", { name: "Triage" })).toBeInTheDocument();
    rerender(
      <QueryClientProvider client={queryClient}>
        <IntakeStatePicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value="99999999-9999-4999-8999-999999999999"
          onChange={() => {}}
        />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Select intake state" })).toBeInTheDocument();
  });

  it("supports icon-only and custom triggers", () => {
    const { queryClient } = harness(TRIAGE);
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <IntakeStatePicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={() => {}} iconOnly />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Select intake state" })).toBeInTheDocument();
    rerender(
      <QueryClientProvider client={queryClient}>
        <IntakeStatePicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value={null}
          onChange={() => {}}
          trigger={<button type="button">Custom intake</button>}
        />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Custom intake" })).toBeInTheDocument();
  });

  it("shows the icon glyph once the fallback loads", async () => {
    const { queryClient } = harness(TRIAGE);
    render(
      <QueryClientProvider client={queryClient}>
        <IntakeStatePicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={() => {}} iconOnly />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select intake state" }));
    await screen.findByRole("option", { name: "Triage" });
    expect(screen.getByRole("button", { name: "Triage" }).querySelector("svg")).not.toBeNull();
  });

  it("follows a controlled open state", async () => {
    const { queryClient } = harness(TRIAGE);
    const onOpenChange = vi.fn();
    renderPicker({ open: true, onOpenChange }, queryClient);
    expect(await screen.findByRole("option", { name: "Triage" })).toBeInTheDocument();
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("does not fetch or open while disabled", () => {
    const { queryClient, calls } = harness(TRIAGE);
    renderPicker({ disabled: true }, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Select intake state" }));
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(calls).toEqual([]);
  });

  it("shows the loading row while fetching", async () => {
    const { queryClient } = harness(TRIAGE, 200, true);
    renderPicker({}, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Select intake state" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Loading intake states");
  });

  it("shows the error row when the read fails", async () => {
    const { queryClient } = harness({}, 500);
    renderPicker({}, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Select intake state" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't load intake states.");
  });

  it("reads a missing triage state as an empty list", async () => {
    const { queryClient } = harness({}, 404);
    renderPicker({}, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Select intake state" }));
    expect(await screen.findByText("No matching results")).toBeInTheDocument();
  });
});
