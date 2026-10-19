// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { ProjectPicker } from "./ProjectPicker.js";

const SEED = "22222222-2222-4222-8222-222222222222";
const OTHER = "33333333-3333-4333-8333-333333333333";
const WORKSPACE = "44444444-4444-4444-8444-444444444444";

function projectRow(id: string, name: string, identifier: string, logoProps: Record<string, unknown> = {}) {
  return {
    id,
    name,
    identifier,
    sort_order: null,
    logo_props: logoProps,
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
  };
}

const PROJECTS = [
  projectRow(SEED, "Seed project", "SEED", { in_use: "emoji", emoji: { value: "🌱" } }),
  projectRow(OTHER, "Other project", "OTH"),
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

describe("ProjectPicker", () => {
  it("fetches lazily with selected-first sort and logo tiles", async () => {
    const { queryClient, calls } = harness(PROJECTS);
    const onChange = vi.fn();
    render(
      <QueryClientProvider client={queryClient}>
        <ProjectPicker workspaceSlug="acme" value={OTHER} onChange={onChange} />
      </QueryClientProvider>
    );
    expect(calls).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Select project" }));
    expect(await screen.findByRole("option", { name: "Seed project" })).toBeInTheDocument();
    expect(optionNames()).toEqual(["Other project", "Seed project"]);
    expect(screen.getByRole("option", { name: "Seed project" })).toHaveTextContent("🌱");
    expect(screen.getByRole("option", { name: "Other project" })).toHaveTextContent("O");
    expect(screen.getByRole("button", { name: "Other project" })).toBeInTheDocument();
    expect(calls.some((url) => url.endsWith("/projects/"))).toBe(true);
  });

  it("searches names, picks and closes", async () => {
    const user = userEvent.setup();
    const { queryClient } = harness(PROJECTS);
    const onChange = vi.fn();
    render(
      <QueryClientProvider client={queryClient}>
        <ProjectPicker workspaceSlug="acme" value={SEED} onChange={onChange} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select project" }));
    expect(await screen.findByRole("option", { name: "Seed project" })).toBeInTheDocument();
    await user.type(screen.getByRole("combobox", { name: "Search projects" }), "other");
    expect(optionNames()).toEqual(["Other project"]);
    fireEvent.click(screen.getByRole("option", { name: "Other project" }));
    expect(onChange).toHaveBeenCalledWith(OTHER);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("names a search miss", async () => {
    const user = userEvent.setup();
    const { queryClient } = harness(PROJECTS);
    render(
      <QueryClientProvider client={queryClient}>
        <ProjectPicker workspaceSlug="acme" value={null} onChange={() => undefined} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select project" }));
    expect(await screen.findByRole("option", { name: "Seed project" })).toBeInTheDocument();
    await user.type(screen.getByRole("combobox", { name: "Search projects" }), "zzz-no-such-project-zzz");
    expect(screen.getByRole("status")).toHaveTextContent("No matching results");
  });

  it("keeps multi-select open, toggles and counts", async () => {
    const { queryClient } = harness(PROJECTS);
    const onChange = vi.fn();
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <ProjectPicker workspaceSlug="acme" multiple value={[]} onChange={onChange} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select projects" }));
    expect(await screen.findByRole("option", { name: "Seed project" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("option", { name: "Seed project" }));
    expect(onChange).toHaveBeenCalledWith([SEED]);
    expect(screen.getByRole("listbox")).toBeInTheDocument();
    rerender(
      <QueryClientProvider client={queryClient}>
        <ProjectPicker workspaceSlug="acme" multiple value={[SEED]} onChange={onChange} />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Seed project" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("option", { name: "Seed project" }));
    expect(onChange).toHaveBeenCalledWith([]);
    rerender(
      <QueryClientProvider client={queryClient}>
        <ProjectPicker workspaceSlug="acme" multiple value={[SEED, OTHER]} onChange={onChange} />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "2 projects" })).toBeInTheDocument();
    rerender(
      <QueryClientProvider client={queryClient}>
        <ProjectPicker workspaceSlug="acme" multiple value={[]} onChange={onChange} iconOnly />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Select projects" })).toBeInTheDocument();
  });

  it("excludes projects on request", async () => {
    const { queryClient } = harness(PROJECTS);
    render(
      <QueryClientProvider client={queryClient}>
        <ProjectPicker workspaceSlug="acme" value={null} onChange={() => undefined} excludeProjectIds={[SEED]} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select project" }));
    expect(await screen.findByRole("option", { name: "Other project" })).toBeInTheDocument();
    expect(optionNames()).toEqual(["Other project"]);
  });

  it("offers only the caller's allow list", async () => {
    const { queryClient } = harness(PROJECTS);
    render(
      <QueryClientProvider client={queryClient}>
        <ProjectPicker workspaceSlug="acme" value={null} onChange={() => undefined} includeProjectIds={[OTHER]} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select project" }));
    expect(await screen.findByRole("option", { name: "Other project" })).toBeInTheDocument();
    expect(optionNames()).toEqual(["Other project"]);
  });

  it("cannot show a row outside the allow list while the read is pending", () => {
    // A pending read leaves no rows at all; an allow list can only narrow
    // them once they land, so nothing outside the list can flash first.
    const { queryClient } = harness(PROJECTS);
    render(
      <QueryClientProvider client={queryClient}>
        <ProjectPicker workspaceSlug="acme" value={null} onChange={() => undefined} includeProjectIds={[OTHER]} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select project" }));
    screen.queryAllByRole("option").forEach((option) => expect(option).toHaveAccessibleName("Other project"));
  });

  it("reports failed loads and blocks reads while disabled", async () => {
    const failed = harness({ detail: "boom" }, 500);
    const first = render(
      <QueryClientProvider client={failed.queryClient}>
        <ProjectPicker workspaceSlug="acme" value={null} onChange={() => undefined} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select project" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't load projects.");
    first.unmount();

    const { queryClient, calls } = harness(PROJECTS);
    render(
      <QueryClientProvider client={queryClient}>
        <ProjectPicker workspaceSlug="acme" value={null} onChange={() => undefined} disabled />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Select project" })).toBeDisabled();
    expect(calls).toEqual([]);
  });

  it("reports open changes to a controlled caller", async () => {
    const { queryClient } = harness(PROJECTS);
    const onOpenChange = vi.fn();
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <ProjectPicker
          workspaceSlug="acme"
          value={null}
          onChange={() => undefined}
          open={false}
          onOpenChange={onOpenChange}
        />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select project" }));
    expect(onOpenChange).toHaveBeenCalledWith(true);
    rerender(
      <QueryClientProvider client={queryClient}>
        <ProjectPicker workspaceSlug="acme" value={null} onChange={() => undefined} open onOpenChange={onOpenChange} />
      </QueryClientProvider>
    );
    expect(await screen.findByRole("option", { name: "Seed project" })).toBeInTheDocument();
  });

  it("counts an unknown selection, overflows long stacks and opens without a handler", async () => {
    const user = userEvent.setup();
    const { queryClient } = harness(PROJECTS);
    const onChange = vi.fn();
    const missing = "99999999-9999-4999-8999-999999999999";
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <ProjectPicker workspaceSlug="acme" multiple value={[missing]} onChange={onChange} />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "1 project" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "1 project" }));
    expect(await screen.findByRole("option", { name: "Seed project" })).toBeInTheDocument();
    await user.click(document.body);
    rerender(
      <QueryClientProvider client={queryClient}>
        <ProjectPicker workspaceSlug="acme" multiple value={[SEED, OTHER, missing]} onChange={onChange} iconOnly />
      </QueryClientProvider>
    );
    const trigger = screen.getByRole("button", { name: "3 projects" });
    expect(trigger).toHaveTextContent("+1");
    rerender(
      <QueryClientProvider client={queryClient}>
        <ProjectPicker workspaceSlug="acme" value={null} onChange={onChange} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select project" }));
    expect(screen.getByRole("listbox", { name: "Projects" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("option", { name: "Seed project" }));
    expect(onChange).toHaveBeenCalledWith(SEED);
  });

  it("labels the picked project from the caller's own rows before the read resolves", () => {
    // The create dialog already holds the projects it may file into, so
    // its trigger must name the pick even while the picker's lazy read
    // has not happened (ISS-117/ISS-123).
    const { queryClient, calls } = harness(PROJECTS);
    const known = [
      {
        id: OTHER,
        name: "Caller's own name",
        identifier: "OWN",
        logoProps: {},
        searchText: "Caller's own name OWN",
        selected: false,
      },
    ];
    render(
      <QueryClientProvider client={queryClient}>
        <ProjectPicker workspaceSlug="acme" value={OTHER} onChange={() => undefined} knownProjects={known} />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Caller's own name" })).toBeInTheDocument();
    expect(calls).toEqual([]);
  });

  it("prefers the read's row over the caller's label once it resolves", async () => {
    const { queryClient } = harness(PROJECTS);
    const known = [
      {
        id: OTHER,
        name: "Caller's own name",
        identifier: "OWN",
        logoProps: {},
        searchText: "Caller's own name OWN",
        selected: false,
      },
    ];
    render(
      <QueryClientProvider client={queryClient}>
        <ProjectPicker workspaceSlug="acme" value={OTHER} onChange={() => undefined} knownProjects={known} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Caller's own name" }));
    expect(await screen.findByRole("option", { name: "Seed project" })).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("button", { name: "Other project" })).toBeInTheDocument());
  });

  it("keeps the read's option list even when the caller's labels name other projects", async () => {
    const { queryClient } = harness(PROJECTS);
    const known = [
      {
        id: SEED,
        name: "Label only",
        identifier: "LBL",
        logoProps: {},
        searchText: "Label only LBL",
        selected: false,
      },
    ];
    render(
      <QueryClientProvider client={queryClient}>
        <ProjectPicker
          workspaceSlug="acme"
          value={null}
          onChange={() => undefined}
          includeProjectIds={[OTHER]}
          knownProjects={known}
        />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select project" }));
    const list = await screen.findByRole("listbox", { name: "Projects" });
    expect(within(list).queryByRole("option", { name: /Label only/ })).not.toBeInTheDocument();
  });

  it("renders icon-only and custom triggers", async () => {
    const { queryClient } = harness(PROJECTS);
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <ProjectPicker workspaceSlug="acme" value={OTHER} onChange={() => undefined} iconOnly />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select project" }));
    expect(await screen.findByRole("option", { name: "Other project" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Other project" })).toHaveTextContent("O");
    rerender(
      <QueryClientProvider client={queryClient}>
        <ProjectPicker
          workspaceSlug="acme"
          value={null}
          onChange={() => undefined}
          trigger={<button type="button">Pick</button>}
          placeholder="Choose"
        />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Pick" })).toBeInTheDocument();
  });
});
