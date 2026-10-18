// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { ModuleChips, ModulePicker } from "./ModulePicker.js";

const ALPHA = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const BETA = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const PROJECT = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

const MODULES = [
  { id: ALPHA, project_id: PROJECT, name: "Alpha", status: "planned", is_favorite: false },
  { id: BETA, project_id: PROJECT, name: "Beta", status: "planned", is_favorite: false },
];

function harness(body: unknown, status = 200) {
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

afterEach(() => {
  resetAppClient();
});

describe("ModulePicker", () => {
  it("fetches lazily and multi-selects with search", async () => {
    const user = userEvent.setup();
    const { queryClient, calls } = harness(MODULES);
    const onChange = vi.fn();
    render(
      <QueryClientProvider client={queryClient}>
        <ModulePicker workspaceSlug="acme" projectId={PROJECT} multiple value={[]} onChange={onChange} />
      </QueryClientProvider>
    );
    expect(calls).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "No module" }));
    expect(await screen.findByRole("option", { name: "Alpha" })).toBeInTheDocument();
    expect(optionNames()).toEqual(["Alpha", "Beta"]);
    expect(calls.some((url) => url.endsWith("/modules/"))).toBe(true);
    await user.type(screen.getByRole("combobox", { name: "Search modules" }), "alp");
    expect(optionNames()).toEqual(["Alpha"]);
    await user.clear(screen.getByRole("combobox", { name: "Search modules" }));
    await user.type(screen.getByRole("combobox", { name: "Search modules" }), "no-such-module");
    expect(screen.getByRole("status")).toHaveTextContent("No matching results");
    await user.clear(screen.getByRole("combobox", { name: "Search modules" }));
    fireEvent.click(screen.getByRole("option", { name: "Alpha" }));
    expect(onChange).toHaveBeenCalledWith([ALPHA]);
    expect(screen.getByRole("listbox")).toBeInTheDocument();
  });

  it("toggles off and counts the trigger", async () => {
    const { queryClient } = harness(MODULES);
    const onChange = vi.fn();
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <ModulePicker workspaceSlug="acme" projectId={PROJECT} multiple value={[ALPHA, BETA]} onChange={onChange} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "2 Modules" }));
    expect(await screen.findByRole("option", { name: "Alpha" })).toBeInTheDocument();
    expect(optionNames()).toEqual(["Alpha", "Beta"]);
    fireEvent.click(screen.getByRole("option", { name: "Alpha" }));
    expect(onChange).toHaveBeenCalledWith([BETA]);
    rerender(
      <QueryClientProvider client={queryClient}>
        <ModulePicker workspaceSlug="acme" projectId={PROJECT} multiple value={[BETA]} onChange={onChange} />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Beta" })).toBeInTheDocument();
    rerender(
      <QueryClientProvider client={queryClient}>
        <ModulePicker workspaceSlug="acme" projectId={PROJECT} multiple value={[]} onChange={onChange} />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "No module" })).toBeInTheDocument();
  });

  it("single-selects with a clear row", async () => {
    const { queryClient } = harness(MODULES);
    const onChange = vi.fn();
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <ModulePicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={onChange} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "No module" }));
    expect(await screen.findByRole("option", { name: "Alpha" })).toBeInTheDocument();
    expect(optionNames()).toEqual(["No module", "Alpha", "Beta"]);
    fireEvent.click(screen.getByRole("option", { name: "Alpha" }));
    expect(onChange).toHaveBeenCalledWith(ALPHA);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    rerender(
      <QueryClientProvider client={queryClient}>
        <ModulePicker workspaceSlug="acme" projectId={PROJECT} value={ALPHA} onChange={onChange} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Alpha" }));
    expect(await screen.findByRole("option", { name: "No module" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("option", { name: "No module" }));
    expect(onChange).toHaveBeenCalledWith(null);
    rerender(
      <QueryClientProvider client={queryClient}>
        <ModulePicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={onChange} allowClear={false} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "No module" }));
    expect(await screen.findByRole("option", { name: "Alpha" })).toBeInTheDocument();
    expect(optionNames()).toEqual(["Alpha", "Beta"]);
  });

  it("reports failed loads and blocks reads while disabled", async () => {
    const failed = harness({ detail: "boom" }, 500);
    const first = render(
      <QueryClientProvider client={failed.queryClient}>
        <ModulePicker workspaceSlug="acme" projectId={PROJECT} multiple value={[]} onChange={() => undefined} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "No module" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't load modules.");
    first.unmount();

    const { queryClient, calls } = harness(MODULES);
    render(
      <QueryClientProvider client={queryClient}>
        <ModulePicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={() => undefined} disabled />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "No module" })).toBeDisabled();
    expect(calls).toEqual([]);
  });

  it("reports open changes to a controlled caller", async () => {
    const { queryClient } = harness(MODULES);
    const onOpenChange = vi.fn();
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <ModulePicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value={null}
          onChange={() => undefined}
          open={false}
          onOpenChange={onOpenChange}
        />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "No module" }));
    expect(onOpenChange).toHaveBeenCalledWith(true);
    rerender(
      <QueryClientProvider client={queryClient}>
        <ModulePicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value={null}
          onChange={() => undefined}
          open
          onOpenChange={onOpenChange}
        />
      </QueryClientProvider>
    );
    expect(await screen.findByRole("option", { name: "Alpha" })).toBeInTheDocument();
  });

  it("counts an unknown module and opens icon-only without a handler", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    const { queryClient } = harness(MODULES);
    const missing = "99999999-9999-4999-8999-999999999999";
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <ModulePicker workspaceSlug="acme" projectId={PROJECT} multiple value={[missing]} onChange={onChange} />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "1 module" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "1 module" }));
    expect(await screen.findByRole("option", { name: "Alpha" })).toBeInTheDocument();
    await user.click(document.body);
    rerender(
      <QueryClientProvider client={queryClient}>
        <ModulePicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={onChange} iconOnly />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "No module" }));
    expect(await screen.findByRole("listbox", { name: "Modules" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("option", { name: "Alpha" }));
    expect(onChange).toHaveBeenCalledWith(ALPHA);
  });

  it("renders icon-only and custom triggers", async () => {
    const user = userEvent.setup();
    const { queryClient } = harness(MODULES);
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <ModulePicker
          workspaceSlug="acme"
          projectId={PROJECT}
          multiple
          value={[ALPHA, BETA]}
          onChange={() => undefined}
          iconOnly
        />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "2 Modules" }));
    expect(await screen.findByRole("option", { name: "Alpha" })).toBeInTheDocument();
    await user.click(document.body);
    rerender(
      <QueryClientProvider client={queryClient}>
        <ModulePicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value={null}
          onChange={() => undefined}
          trigger={<button type="button">Pick</button>}
        />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Pick" })).toBeInTheDocument();
  });
});

describe("ModuleChips", () => {
  it("removes through the chip buttons", () => {
    const onRemove = vi.fn();
    render(
      <ModuleChips
        modules={[
          { id: ALPHA, name: "Alpha" },
          { id: BETA, name: "Beta" },
        ]}
        onRemove={onRemove}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "Remove Alpha" }));
    expect(onRemove).toHaveBeenCalledWith(ALPHA);
  });

  it("renders static chips without a remove handler", () => {
    render(<ModuleChips modules={[{ id: ALPHA, name: "Alpha" }]} />);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.getByText("Alpha")).toBeInTheDocument();
  });

  it("disables removal on request", () => {
    render(<ModuleChips modules={[{ id: ALPHA, name: "Alpha" }]} onRemove={() => undefined} disabled />);
    expect(screen.getByRole("button", { name: "Remove Alpha" })).toBeDisabled();
  });
});
