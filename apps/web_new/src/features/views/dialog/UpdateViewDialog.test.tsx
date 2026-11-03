// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { SavedView } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const notify = vi.hoisted(() => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));
vi.mock("../../../shared/shell/index.js", () => notify);

const session = vi.hoisted(() => ({ useMe: vi.fn(), usePermissions: vi.fn() }));
vi.mock("../../../core/session/hooks.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../core/session/hooks.js")>()),
  useMe: session.useMe,
  usePermissions: session.usePermissions,
}));

const roles = vi.hoisted(() => ({ useProjectRoles: vi.fn() }));
vi.mock("../../auth/queries.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../auth/queries.js")>()),
  useProjectRoles: roles.useProjectRoles,
}));

const router = vi.hoisted(() => ({ navigate: vi.fn() }));
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useNavigate: () => router.navigate,
}));

const harness = vi.hoisted(() => ({
  edition: { id: "oss", flags: {} } as {
    id: string;
    flags: Record<string, boolean>;
    slots?: Record<string, unknown>;
  },
}));
vi.mock("../../../core/edition/index.js", () => ({
  get edition() {
    return harness.edition;
  },
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { loadRichFilterBuilder } from "../../issues/index.js";
import { viewKeys } from "../api/keys.js";
import { UpdateViewDialog } from "../index.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const VIEW_ID = "323e4567-e89b-12d3-a456-426614174002";
const OWNER = "423e4567-e89b-12d3-a456-426614174003";
const WORKSPACE_ID = "523e4567-e89b-12d3-a456-426614174004";

function record(overrides: Partial<SavedView> = {}): SavedView {
  return {
    id: VIEW_ID,
    workspace: WORKSPACE_ID,
    project: PROJECT,
    name: "Triage",
    description: "board with icon",
    query: {},
    filters: {},
    display_filters: { layout: "kanban", group_by: "state" },
    display_properties: {},
    rich_filters: {},
    access: 1,
    sort_order: 1,
    logo_props: { in_use: "icon", icon: { name: "Flag", color: "#5b5bd6" } },
    owned_by: OWNER,
    is_locked: false,
    archived_at: null,
    created_by: OWNER,
    updated_by: OWNER,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

interface Call {
  method: string;
  url: string;
  body: Record<string, unknown> | null;
}

function seedSession(): void {
  session.useMe.mockReturnValue({ data: { id: OWNER } });
  session.usePermissions.mockReturnValue({ isAdmin: false, isLoading: false });
  roles.useProjectRoles.mockReturnValue({ data: undefined });
}

function transport(respond: (call: Call) => { status: number; body: unknown }): Call[] {
  const calls: Call[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
    }
    const call: Call = {
      method: init.method ?? "GET",
      url,
      body: init.body === undefined ? null : (JSON.parse(init.body) as Record<string, unknown>),
    };
    calls.push(call);
    const { status, body } = respond(call);
    return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
  }) as never);
  return calls;
}

function renderDialog(view: SavedView): {
  onOpenChange: ReturnType<typeof vi.fn>;
  queryClient: QueryClient;
} {
  const onOpenChange = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData<SavedView[]>(viewKeys.projectList("acme", PROJECT), [view]);
  render(
    <QueryClientProvider client={queryClient}>
      <UpdateViewDialog
        open={true}
        onOpenChange={onOpenChange}
        workspaceSlug="acme"
        projectId={PROJECT}
        hideCycle={false}
        hideModule={false}
        view={view}
      />
    </QueryClientProvider>
  );
  return { onOpenChange, queryClient };
}

afterEach(() => {
  resetAppClient();
  harness.edition = { id: "oss", flags: {} };
  notify.notifyError.mockClear();
  notify.notifySuccess.mockClear();
  router.navigate.mockClear();
  session.useMe.mockReset();
  session.usePermissions.mockReset();
  roles.useProjectRoles.mockReset();
});

describe("UpdateViewDialog prefill", () => {
  it("opens on the record's title, description, icon and layout", async () => {
    seedSession();
    transport(() => ({ status: 200, body: record() }));
    renderDialog(record());

    expect(await screen.findByRole("heading", { name: "Update View" })).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Title")).toHaveValue("Triage");
    expect(screen.getByPlaceholderText("Description")).toHaveValue("board with icon");
    expect(screen.getByRole("button", { name: "Update View" })).toBeInTheDocument();
    // The stored kanban layout preselects the Board trigger.
    expect(screen.getByRole("button", { name: "Board" })).toBeInTheDocument();
    expect(screen.queryByTestId("view-access-field")).not.toBeInTheDocument();
    expect(screen.queryByTestId("view-access-summary")).not.toBeInTheDocument();
  });

  it("shows the stored access read-only on cloud", async () => {
    seedSession();
    transport(() => ({ status: 200, body: record() }));
    harness.edition = { id: "cloud", flags: {}, slots: { viewAccess: {} } };
    renderDialog(record({ access: 0 }));

    expect(await screen.findByTestId("view-access-summary")).toHaveTextContent("Shared with: Private");
    expect(screen.queryByTestId("view-access-field")).not.toBeInTheDocument();
  });

  it("seeds the filter builder from the stored expression", async () => {
    seedSession();
    transport(() => ({ status: 200, body: record() }));
    renderDialog(record({ rich_filters: { priority__in: "urgent" } }));

    expect(await screen.findByRole("heading", { name: "Update View" })).toBeInTheDocument();
    expect(await screen.findByTestId("rich-filter-row")).toBeInTheDocument();
  });
});

describe("UpdateViewDialog validation", () => {
  it("rejects an empty title without writing", async () => {
    const user = userEvent.setup();
    seedSession();
    const calls = transport(() => ({ status: 200, body: record() }));
    const { onOpenChange } = renderDialog(record());

    await user.clear(screen.getByPlaceholderText("Title"));
    await user.click(screen.getByRole("button", { name: "Update View" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Title is required");
    expect(calls).toHaveLength(0);
    expect(onOpenChange).not.toHaveBeenCalled();
  });
});

describe("UpdateViewDialog submit", () => {
  it("saves silently, closes, and adopts the echo without navigating", async () => {
    const user = userEvent.setup();
    seedSession();
    const calls = transport((call) => ({ status: 200, body: { ...record(), ...(call.body ?? {}) } }));
    const { onOpenChange, queryClient } = renderDialog(record());

    await user.clear(screen.getByPlaceholderText("Title"));
    await user.type(screen.getByPlaceholderText("Title"), "Triage v2");
    await user.clear(screen.getByPlaceholderText("Description"));
    await user.type(screen.getByPlaceholderText("Description"), "after");
    await user.click(screen.getByRole("button", { name: "Update View" }));
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    expect(calls).toHaveLength(1);
    const patch = calls[0]!;
    expect(patch.method).toBe("PATCH");
    expect(patch.url).toContain(`/api/workspaces/acme/projects/${PROJECT}/views/${VIEW_ID}/`);
    expect(patch.body).toMatchObject({ name: "Triage v2", description: "after" });
    expect(patch.body).not.toHaveProperty("access");
    // Silent save: no notice either way, and the list stays mounted.
    expect(notify.notifySuccess).not.toHaveBeenCalled();
    expect(notify.notifyError).not.toHaveBeenCalled();
    expect(router.navigate).not.toHaveBeenCalled();
    const rows = queryClient.getQueryData<SavedView[]>(viewKeys.projectList("acme", PROJECT)) ?? [];
    expect(rows.map((row) => row.name)).toEqual(["Triage v2"]);
    const { savedViewKeys } = await loadRichFilterBuilder();
    expect(queryClient.getQueryData(savedViewKeys.project("acme", PROJECT, VIEW_ID))).toMatchObject({
      id: VIEW_ID,
      name: "Triage v2",
    });
  });

  it("round-trips the stored display, logo and expression untouched", async () => {
    const user = userEvent.setup();
    seedSession();
    const calls = transport((call) => ({ status: 200, body: { ...record(), ...(call.body ?? {}) } }));
    renderDialog(record({ rich_filters: { priority__in: "urgent" } }));

    // Wait for the seeded builder before submitting, so the expression
    // the record carries is what the payload snapshots.
    expect(await screen.findByTestId("rich-filter-row")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Update View" }));
    await waitFor(() => expect(calls).toHaveLength(1));
    expect(calls[0]!.body).toMatchObject({
      name: "Triage",
      display_filters: expect.objectContaining({ layout: "kanban", group_by: "state" }),
      rich_filters: { priority__in: "urgent" },
      logo_props: { in_use: "icon", icon: { name: "Flag", color: "#5b5bd6" } },
    });
  });

  it("omits the logo key when the record carries no logo", async () => {
    const user = userEvent.setup();
    seedSession();
    const calls = transport((call) => ({ status: 200, body: { ...record(), ...(call.body ?? {}) } }));
    renderDialog(record({ logo_props: {} }));

    await user.click(screen.getByRole("button", { name: "Update View" }));
    await waitFor(() => expect(calls).toHaveLength(1));
    expect(calls[0]!.body).not.toHaveProperty("logo_props");
  });

  it("restores the prefilled originals when the write fails", async () => {
    const user = userEvent.setup();
    seedSession();
    transport(() => ({ status: 500, body: {} }));
    const { onOpenChange, queryClient } = renderDialog(record());

    await user.clear(screen.getByPlaceholderText("Title"));
    await user.type(screen.getByPlaceholderText("Title"), "Triage v3");
    await user.clear(screen.getByPlaceholderText("Description"));
    await user.type(screen.getByPlaceholderText("Description"), "doomed");
    await user.click(screen.getByRole("button", { name: "Board" }));
    await user.click(screen.getByRole("option", { name: "List" }));
    await user.click(screen.getByRole("button", { name: "Update View" }));
    await waitFor(() =>
      expect(notify.notifyError).toHaveBeenCalledWith(
        "Failed to update view",
        "Nothing was changed — check the details and try again."
      )
    );
    // The dialog stays open on the prefilled originals; nothing saved is lost.
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(screen.getByPlaceholderText("Title")).toHaveValue("Triage");
    expect(screen.getByPlaceholderText("Description")).toHaveValue("board with icon");
    expect(screen.getByRole("button", { name: "Board" })).toBeInTheDocument();
    const rows = queryClient.getQueryData<SavedView[]>(viewKeys.projectList("acme", PROJECT)) ?? [];
    expect(rows.map((row) => row.name)).toEqual(["Triage"]);
  });

  it("names the server detail on failure", async () => {
    const user = userEvent.setup();
    seedSession();
    transport(() => ({ status: 400, body: { detail: "Locked views cannot be edited" } }));
    renderDialog(record());

    await user.type(screen.getByPlaceholderText("Title"), " v2");
    await user.click(screen.getByRole("button", { name: "Update View" }));
    await waitFor(() =>
      expect(notify.notifyError).toHaveBeenCalledWith("Failed to update view", "Locked views cannot be edited")
    );
  });

  it("shows a pending label while the write flies", async () => {
    const user = userEvent.setup();
    seedSession();
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
      }
      await gate;
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(record()) };
    }) as never);
    const { onOpenChange } = renderDialog(record());

    await user.click(screen.getByRole("button", { name: "Update View" }));
    expect(await screen.findByRole("button", { name: "Updating view…" })).toBeDisabled();
    release();
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
  });
});
