// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../../core/api/client.js";

vi.mock("../../../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

const session = vi.hoisted(() => ({
  viewer: { data: { id: "123e4567-e89b-12d3-a456-426614174000" } } as { data?: { id: string } | undefined },
}));

vi.mock("../../../../../core/session/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../../../core/session/index.js")>();
  return { ...original, useMe: () => session.viewer };
});

import { notifyError, notifySuccess } from "../../../../../shared/shell/index.js";
import { WorklogSidebarSection } from "./WorklogSidebarSection.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "323e4567-e89b-12d3-a456-426614174002";
const VIEWER = "123e4567-e89b-12d3-a456-426614174000";
const OTHER = "523e4567-e89b-12d3-a456-426614174004";
const ENTRY_ONE = "423e4567-e89b-12d3-a456-426614174003";
const ENTRY_TWO = "623e4567-e89b-12d3-a456-426614174005";

function entryRow(overrides: Record<string, unknown> = {}) {
  return {
    id: ENTRY_ONE,
    issue: ISSUE,
    project: PROJECT,
    workspace: PROJECT,
    actor: VIEWER,
    actor_detail: {
      id: VIEWER,
      first_name: "Viewer",
      last_name: "Self",
      avatar: "",
      avatar_url: null,
      display_name: "viewer.self",
      is_bot: false,
    },
    minutes: 90,
    logged_on: "2026-10-20",
    note: "Debugging the flake",
    created_at: "2026-10-20T22:00:00Z",
    updated_at: "2026-10-20T22:00:00Z",
    created_by: VIEWER,
    updated_by: VIEWER,
    deleted_at: null,
    ...overrides,
  };
}

interface StubOptions {
  rows?: Record<string, unknown>[];
  listStatus?: number;
  patchStatus?: number;
  deleteStatus?: number;
}

function stubTransport(options: StubOptions = {}) {
  const calls: { url: string; method: string; body: string }[] = [];
  let rows = options.rows ?? [entryRow()];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    calls.push({ url, method: init.method ?? "GET", body: init.body ?? "" });
    const json = (status: number, body: unknown) => ({
      status,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(body),
    });
    if (url.endsWith("/auth/get-csrf-token/")) return json(200, { csrf_token: "csrf" });
    if (url.includes(`/issues/${ISSUE}/worklogs/`)) {
      const method = init.method ?? "GET";
      if (method === "GET" && url.endsWith("/worklogs/")) {
        if (options.listStatus !== undefined) return json(options.listStatus, { error: "down" });
        return json(200, rows);
      }
      if (method === "PATCH") {
        if (options.patchStatus !== undefined) return json(options.patchStatus, { error: "refused" });
        const body = JSON.parse(init.body ?? "{}") as Record<string, unknown>;
        rows = rows.map((row) => (url.includes(`/${row["id"]}/`) ? { ...row, ...body } : row));
        return json(
          200,
          rows.find((row) => url.includes(`/${row["id"]}/`))
        );
      }
      if (method === "DELETE") {
        if (options.deleteStatus !== undefined) return json(options.deleteStatus, { error: "refused" });
        rows = rows.filter((row) => !url.includes(`/${row["id"]}/`));
        return json(204, {});
      }
    }
    throw new Error(`unexpected ${init.method ?? "GET"} ${url}`);
  }) as never);
  return { calls };
}

function setup(options: StubOptions = {}, overrides: Partial<React.ComponentProps<typeof WorklogSidebarSection>> = {}) {
  const stub = stubTransport(options);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const invalidate = vi.spyOn(queryClient, "invalidateQueries");
  render(
    <QueryClientProvider client={queryClient}>
      <WorklogSidebarSection
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issueId={ISSUE}
        viewerId={VIEWER}
        archived={false}
        {...overrides}
      />
    </QueryClientProvider>
  );
  return { ...stub, invalidate };
}

beforeEach(() => vi.clearAllMocks());
afterEach(() => resetAppClient());

describe("WorklogSidebarSection", () => {
  it("shows the total and one row per entry", async () => {
    setup({
      rows: [
        entryRow(),
        entryRow({
          id: ENTRY_TWO,
          actor: OTHER,
          actor_detail: {
            id: OTHER,
            first_name: "Ada",
            last_name: "L",
            avatar: "",
            avatar_url: null,
            display_name: "Ada L",
            is_bot: false,
          },
          minutes: 30,
          logged_on: "2026-10-19",
          note: "",
        }),
      ],
    });

    expect(await screen.findByText("2h")).toBeDefined();
    expect(screen.getByText("1h 30m")).toBeDefined();
    expect(screen.getByText("Debugging the flake")).toBeDefined();
    expect(screen.getByText(/Ada L/)).toBeDefined();
    expect(screen.queryByText("viewer.self")).toBeNull();
  });

  it("reads an empty collection as no time logged", async () => {
    setup({ rows: [] });
    expect(await screen.findByText("No time logged yet.")).toBeDefined();
    expect(screen.queryByText("0m")).toBeNull();
  });

  it("retries a failed read", async () => {
    const user = userEvent.setup();
    let attempts = 0;
    const calls: { url: string; method: string }[] = [];
    createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
      calls.push({ url, method: init.method ?? "GET" });
      const json = (status: number, body: unknown) => ({
        status,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(body),
      });
      if (url.endsWith("/auth/get-csrf-token/")) return json(200, { csrf_token: "csrf" });
      attempts += 1;
      if (attempts === 1) return json(500, { error: "down" });
      return json(200, [entryRow()]);
    }) as never);
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <WorklogSidebarSection
          workspaceSlug={SLUG}
          projectId={PROJECT}
          issueId={ISSUE}
          viewerId={VIEWER}
          archived={false}
        />
      </QueryClientProvider>
    );

    expect(await screen.findByText("Couldn't load the worklog.")).toBeDefined();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText("1h 30m", { selector: "[data-worklog-total]" })).toBeDefined();
  });

  it("edits an entry through the dialog and refreshes the feed key", async () => {
    const user = userEvent.setup();
    const { calls, invalidate } = setup();

    await user.click(await screen.findByRole("button", { name: "Edit 1h 30m on Oct 20, 2026" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit time entry" });
    await user.clear(within(dialog).getByLabelText("Minutes"));
    await user.type(within(dialog).getByLabelText("Minutes"), "45");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));

    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Time entry updated"));
    expect(screen.queryByRole("dialog")).toBeNull();
    const patch = calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toContain(`/worklogs/${ENTRY_ONE}/`);
    expect(JSON.parse(patch?.body ?? "{}")).toEqual({
      minutes: 45,
      logged_on: "2026-10-20",
      note: "Debugging the flake",
    });
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["ws", SLUG, "issues", "activity", PROJECT, ISSUE],
    });
  });

  it("keeps the edit dialog open when the server refuses", async () => {
    const user = userEvent.setup();
    setup({ patchStatus: 403 });

    await user.click(await screen.findByRole("button", { name: "Edit 1h 30m on Oct 20, 2026" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit time entry" });
    await user.click(within(dialog).getByRole("button", { name: "Save" }));

    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't update the time entry"));
    expect(screen.getByRole("dialog")).toBeDefined();
  });

  it("confirms with the duration and day, then removes the entry", async () => {
    const user = userEvent.setup();
    const { calls } = setup();

    await user.click(await screen.findByRole("button", { name: "Remove 1h 30m on Oct 20, 2026" }));
    const dialog = await screen.findByRole("dialog", { name: "Remove 1h 30m on Oct 20, 2026?" });
    expect(dialog).toHaveTextContent("permanently removed");
    await user.click(within(dialog).getByRole("button", { name: "Remove" }));

    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Time entry removed"));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(calls.some((call) => call.method === "DELETE" && call.url.includes(`/worklogs/${ENTRY_ONE}/`))).toBe(true);
  });

  it("cancels a removal without a request", async () => {
    const user = userEvent.setup();
    const { calls } = setup();

    await user.click(await screen.findByRole("button", { name: "Remove 1h 30m on Oct 20, 2026" }));
    const dialog = await screen.findByRole("dialog", { name: "Remove 1h 30m on Oct 20, 2026?" });
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(calls.some((call) => call.method === "DELETE")).toBe(false);
  });

  it("keeps the confirm open when the server refuses a removal", async () => {
    const user = userEvent.setup();
    setup({ deleteStatus: 403 });

    await user.click(await screen.findByRole("button", { name: "Remove 1h 30m on Oct 20, 2026" }));
    const dialog = await screen.findByRole("dialog", { name: "Remove 1h 30m on Oct 20, 2026?" });
    await user.click(within(dialog).getByRole("button", { name: "Remove" }));

    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Couldn't remove the time entry"));
    expect(screen.getByRole("dialog")).toBeDefined();
  });

  it("hides manage controls for others' entries", async () => {
    setup({ rows: [entryRow({ actor: OTHER, actor_detail: null })] });
    expect(await screen.findByText(/Someone/)).toBeDefined();
    expect(screen.queryByRole("button", { name: /Edit / })).toBeNull();
    expect(screen.queryByRole("button", { name: /Remove / })).toBeNull();
  });

  it("hides manage controls on archived items", async () => {
    setup({}, { archived: true });
    expect(await screen.findByText("1h 30m", { selector: "[data-worklog-total]" })).toBeDefined();
    expect(screen.queryByRole("button", { name: /Edit / })).toBeNull();
    expect(screen.queryByRole("button", { name: /Remove / })).toBeNull();
  });

  it("falls back to the session viewer without an override", async () => {
    session.viewer = { data: { id: VIEWER } };
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    stubTransport();
    render(
      <QueryClientProvider client={queryClient}>
        <WorklogSidebarSection workspaceSlug={SLUG} projectId={PROJECT} issueId={ISSUE} archived={false} />
      </QueryClientProvider>
    );
    expect(await screen.findByRole("button", { name: "Edit 1h 30m on Oct 20, 2026" })).toBeDefined();
  });

  it("reads no manage controls when neither override nor session names a viewer", async () => {
    session.viewer = { data: undefined };
    try {
      const queryClient = new QueryClient({
        defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
      });
      stubTransport();
      render(
        <QueryClientProvider client={queryClient}>
          <WorklogSidebarSection workspaceSlug={SLUG} projectId={PROJECT} issueId={ISSUE} archived={false} />
        </QueryClientProvider>
      );
      expect(await screen.findByText("1h 30m", { selector: "[data-worklog-total]" })).toBeDefined();
      expect(screen.queryByRole("button", { name: /Edit / })).toBeNull();
      expect(screen.queryByRole("button", { name: /Remove / })).toBeNull();
    } finally {
      session.viewer = { data: { id: VIEWER } };
    }
  });
});
