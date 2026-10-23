// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ToastHost } from "@pidash/kit";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { getToastManager } from "../../../shared/shell/index.js";
import { ExportsForm } from "./ExportsForm.js";

const WORKSPACE = "44444444-4444-4444-8444-444444444444";

function projectRow(id: string, name: string, identifier: string, overrides = {}) {
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

const PROJECT_A = projectRow("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "Exp Alpha", "EA");
const PROJECT_B = projectRow("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", "Exp Beta", "EB");

interface HarnessOptions {
  projects?: unknown[];
  postStatus?: number;
  gatePost?: boolean;
}

interface Harness {
  posts: Array<{ url: string; body: unknown }>;
  releasePost: () => void;
}

function harness({
  projects = [PROJECT_A, PROJECT_B],
  postStatus = 200,
  gatePost = false,
}: HarnessOptions = {}): Harness {
  const posts: Array<{ url: string; body: unknown }> = [];
  let releasePost = () => {};
  const gate = gatePost
    ? new Promise<void>((resolve) => {
        releasePost = resolve;
      })
    : null;
  createAppClient("https://api.test", (async (url: string, init: { method: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    if (url.endsWith("/projects/")) {
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify(projects),
      };
    }
    if (url.endsWith("/export-issues/")) {
      if (gate !== null) await gate;
      posts.push({ url, body: JSON.parse(init.body ?? "{}") });
      return {
        status: postStatus,
        url,
        headers: { get: () => "application/json" },
        text: async () =>
          postStatus === 200 ? JSON.stringify({ message: "queued" }) : JSON.stringify({ error: "boom" }),
      };
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
  return { posts, releasePost };
}

function renderForm(): void {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={queryClient}>
      <ExportsForm workspaceSlug="acme" />
      <ToastHost manager={getToastManager()} />
    </QueryClientProvider>
  );
}

function optionNames(): string[] {
  return within(screen.getByRole("listbox"))
    .getAllByRole("option")
    .map((row) => row.querySelector(":scope > span.truncate")?.textContent ?? "");
}

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
});

describe("ExportsForm", () => {
  it("renders the titled boxes with the default picks", () => {
    harness();
    renderForm();

    expect(screen.getByRole("heading", { name: "Exporting project" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Format" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "All projects" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "CSV" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Export" })).toBeEnabled();
  });

  it("lists only creatable projects and searches by name and key", async () => {
    const user = userEvent.setup();
    harness({
      projects: [
        PROJECT_A,
        PROJECT_B,
        projectRow("cccccccc-cccc-4ccc-8ccc-cccccccccccc", "Exp Guest", "EG", { member_role: 5 }),
        projectRow("dddddddd-dddd-4ddd-8ddd-dddddddddddd", "Exp Outside", "EO", { member_role: null }),
        projectRow("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee", "Exp Archived", "ER", {
          archived_at: "2026-02-01T00:00:00.000Z",
        }),
      ],
    });
    renderForm();

    await user.click(screen.getByRole("button", { name: "All projects" }));
    expect(await screen.findByRole("option", { name: "Exp Alpha" })).toBeInTheDocument();
    expect(optionNames()).toEqual(["Exp Alpha", "Exp Beta"]);

    const search = screen.getByRole("combobox", { name: "Search projects" });
    await user.type(search, "Beta");
    expect(optionNames()).toEqual(["Exp Beta"]);
    await user.clear(search);
    await user.type(search, "EA");
    expect(optionNames()).toEqual(["Exp Alpha"]);
    await user.clear(search);
    expect(optionNames()).toEqual(["Exp Alpha", "Exp Beta"]);
  });

  it("compacts picks to keys and restores All projects on clear", async () => {
    const user = userEvent.setup();
    harness();
    renderForm();

    await user.click(screen.getByRole("button", { name: "All projects" }));
    await user.click(await screen.findByRole("option", { name: "Exp Alpha" }));
    expect(screen.getByRole("button", { name: "EA" })).toBeInTheDocument();
    await user.click(screen.getByRole("option", { name: "Exp Beta" }));
    expect(screen.getByRole("button", { name: "EA, EB" })).toBeInTheDocument();
    await user.click(screen.getByRole("option", { name: "Exp Alpha" }));
    expect(screen.getByRole("button", { name: "EB" })).toBeInTheDocument();
    await user.click(screen.getByRole("option", { name: "Exp Beta" }));
    expect(screen.getByRole("button", { name: "All projects" })).toBeInTheDocument();
  });

  it("keeps an enabled empty picker for a member with no creatable project", async () => {
    const user = userEvent.setup();
    harness({
      projects: [
        projectRow("cccccccc-cccc-4ccc-8ccc-cccccccccccc", "Exp Guest", "EG", { member_role: 5 }),
        projectRow("dddddddd-dddd-4ddd-8ddd-dddddddddddd", "Exp Outside", "EO", { member_role: null }),
      ],
    });
    renderForm();

    const trigger = screen.getByRole("button", { name: "All projects" });
    expect(trigger).toBeEnabled();
    await user.click(trigger);
    expect(await screen.findByText("No matching results")).toBeInTheDocument();
    expect(screen.queryByRole("option")).not.toBeInTheDocument();
  });

  it("offers the three formats and sends the chosen one", async () => {
    const user = userEvent.setup();
    const { posts } = harness();
    renderForm();

    await user.click(screen.getByRole("button", { name: "CSV" }));
    expect(await screen.findByRole("option", { name: "JSON" })).toBeInTheDocument();
    expect(optionNames()).toEqual(["CSV", "Excel", "JSON"]);
    await user.click(screen.getByRole("option", { name: "JSON" }));
    expect(screen.getByRole("button", { name: "JSON" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Export" }));
    await waitFor(() => expect(posts).toHaveLength(1));
    expect(posts[0]?.body).toEqual({ provider: "json", project: [], multiple: false });
  });

  it("works in flight, confirms with the format, and stays usable", async () => {
    const user = userEvent.setup();
    const { posts, releasePost } = harness({ gatePost: true });
    renderForm();

    await user.click(screen.getByRole("button", { name: "Export" }));
    expect(await screen.findByRole("button", { name: "Exporting..." })).toBeDisabled();
    releasePost();
    await waitFor(() => expect(screen.getByTestId("toast-stack")).toHaveTextContent("Export successful"));
    expect(screen.getByTestId("toast-stack")).toHaveTextContent("Your CSV export has started.");
    expect(screen.getByRole("button", { name: "Export" })).toBeEnabled();
    expect(posts).toHaveLength(1);
    expect(posts[0]?.body).toEqual({ provider: "csv", project: [], multiple: false });
    expect(posts[0]?.url).toBe("https://api.test/api/workspaces/acme/export-issues/");
  });

  it("sends picked ids with the multi flag", async () => {
    const user = userEvent.setup();
    const { posts } = harness();
    renderForm();

    await user.click(screen.getByRole("button", { name: "All projects" }));
    await user.click(await screen.findByRole("option", { name: "Exp Alpha" }));
    await user.click(screen.getByRole("option", { name: "Exp Beta" }));
    await user.click(screen.getByRole("button", { name: "Export" }));
    await waitFor(() => expect(posts).toHaveLength(1));
    expect(posts[0]?.body).toEqual({
      provider: "csv",
      project: [PROJECT_A.id, PROJECT_B.id],
      multiple: true,
    });
  });

  it("ignores a second submit while one flies", async () => {
    const { posts, releasePost } = harness({ gatePost: true });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    const { container } = render(
      <QueryClientProvider client={queryClient}>
        <ExportsForm workspaceSlug="acme" />
        <ToastHost manager={getToastManager()} />
      </QueryClientProvider>
    );

    const form = container.querySelector("form") ?? document.createElement("form");
    fireEvent.submit(form);
    fireEvent.submit(form);
    releasePost();
    await waitFor(() => expect(screen.getByTestId("toast-stack")).toHaveTextContent("Export successful"));
    expect(posts).toHaveLength(1);
  });

  it("prompts a retry on failure and stays usable", async () => {
    const user = userEvent.setup();
    const { posts } = harness({ postStatus: 500 });
    renderForm();

    await user.click(screen.getByRole("button", { name: "Export" }));
    await waitFor(() => expect(screen.getByTestId("toast-stack")).toHaveTextContent("Error"));
    expect(screen.getByTestId("toast-stack")).toHaveTextContent("Export was unsuccessful. Please try again.");
    expect(posts).toHaveLength(1);
    const submit = screen.getByRole("button", { name: "Export" });
    expect(submit).toBeEnabled();
  });

  it("reaches the submit by keyboard and fires on Enter", async () => {
    const user = userEvent.setup();
    const { posts } = harness();
    renderForm();

    await user.tab();
    expect(screen.getByRole("button", { name: "All projects" })).toHaveFocus();
    await user.tab();
    expect(screen.getByRole("button", { name: "CSV" })).toHaveFocus();
    await user.tab();
    const submit = screen.getByRole("button", { name: "Export" });
    expect(submit).toHaveFocus();
    await user.keyboard("{Enter}");
    await waitFor(() => expect(posts).toHaveLength(1));
    expect(posts[0]?.body).toEqual({ provider: "csv", project: [], multiple: false });
  });
});
