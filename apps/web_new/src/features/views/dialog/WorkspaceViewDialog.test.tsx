// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { SavedView } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, type Mock, vi } from "vitest";

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
import type { ViewAccessFieldProps } from "../../../core/edition/index.js";
import { loadRichFilterBuilder } from "../../issues/index.js";
import { viewKeys } from "../api/keys.js";
import { WorkspaceViewDialog, type WorkspaceViewDialogProps } from "../index.js";
import { makeView, VIEWS_OWNER_ID } from "../testing.js";

const VIEW_ID = "323e4567-e89b-12d3-a456-426614174002";
const OTHER_ID = "623e4567-e89b-12d3-a456-426614174005";

const CREATED = makeView({
  id: VIEW_ID,
  project: null,
  name: "Triage",
  description: "spreadsheet view",
  display_filters: { layout: "spreadsheet", order_by: "-created_at" },
});

const STORED = makeView({
  id: VIEW_ID,
  project: null,
  name: "Alpha",
  description: "Before desc",
  display_filters: { layout: "list", group_by: "priority", order_by: "-updated_at" },
  display_properties: { priority: true },
  rich_filters: { priority__in: "high" },
});

interface Call {
  method: string;
  url: string;
  body: Record<string, unknown> | null;
}

function seedSession(): void {
  session.useMe.mockReturnValue({ data: { id: VIEWS_OWNER_ID } });
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

function renderDialog(
  options: { open?: boolean; onOpenChange?: (open: boolean) => void } & (
    | { mode: "create" }
    | { mode: "edit"; view: SavedView }
  )
): {
  onOpenChange: Mock<(open: boolean) => void>;
  queryClient: QueryClient;
  rerenderOpen: (open: boolean) => void;
} {
  const onOpenChange = (options.onOpenChange ?? vi.fn()) as Mock<(open: boolean) => void>;
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const ui = (open: boolean): React.ReactElement => {
    const props: WorkspaceViewDialogProps =
      options.mode === "create"
        ? { open, onOpenChange, workspaceSlug: "acme", mode: "create" }
        : { open, onOpenChange, workspaceSlug: "acme", mode: "edit", view: options.view };
    return (
      <QueryClientProvider client={queryClient}>
        <WorkspaceViewDialog {...props} />
      </QueryClientProvider>
    );
  };
  const view = render(ui(options.open ?? true));
  return {
    onOpenChange,
    queryClient,
    rerenderOpen: (open: boolean) => {
      view.rerender(ui(open));
    },
  };
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

describe("WorkspaceViewDialog create blank form", () => {
  it("opens with defaults, a spreadsheet display and an expanded builder", async () => {
    seedSession();
    transport(() => ({ status: 201, body: CREATED }));
    renderDialog({ mode: "create" });
    expect(screen.getByRole("heading", { name: "Create View" })).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Title")).toHaveValue("");
    expect(screen.getByPlaceholderText("Description")).toHaveValue("");
    // No layout picker on the workspace dialog (VIEW-034 fixes spreadsheet).
    expect(screen.queryByRole("button", { name: "List" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Board" })).not.toBeInTheDocument();
    expect(screen.getByTestId("display-options-trigger")).toBeInTheDocument();
    expect(await screen.findByTestId("rich-filter-add")).toBeInTheDocument();
    expect(screen.queryByTestId("view-access-field")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create View" })).toHaveAttribute("type", "submit");
  });

  it("focuses the title on open", () => {
    seedSession();
    transport(() => ({ status: 201, body: CREATED }));
    renderDialog({ mode: "create" });
    expect(screen.getByPlaceholderText("Title")).toHaveFocus();
  });
});

describe("WorkspaceViewDialog validation", () => {
  it("requires a title", async () => {
    const user = userEvent.setup();
    seedSession();
    const calls = transport(() => ({ status: 201, body: CREATED }));
    const { onOpenChange } = renderDialog({ mode: "create" });
    await user.click(screen.getByRole("button", { name: "Create View" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Title is required");
    expect(calls).toHaveLength(0);
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("caps the title at 255 characters", async () => {
    const user = userEvent.setup();
    seedSession();
    const calls = transport(() => ({ status: 201, body: CREATED }));
    renderDialog({ mode: "create" });
    await user.type(screen.getByPlaceholderText("Title"), "x".repeat(256));
    await user.click(screen.getByRole("button", { name: "Create View" }));
    expect(screen.getByRole("alert").textContent).toContain("255");
    expect(calls).toHaveLength(0);
  });

  it("clears the error while retyping", async () => {
    const user = userEvent.setup();
    seedSession();
    transport(() => ({ status: 201, body: CREATED }));
    renderDialog({ mode: "create" });
    await user.click(screen.getByRole("button", { name: "Create View" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Title is required");
    await user.type(screen.getByPlaceholderText("Title"), "Triage");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("requires a title in edit mode too", async () => {
    const user = userEvent.setup();
    seedSession();
    const calls = transport(() => ({ status: 200, body: STORED }));
    renderDialog({ mode: "edit", view: STORED });
    await user.clear(screen.getByPlaceholderText("Title"));
    await user.click(screen.getByRole("button", { name: "Update View" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Title is required");
    expect(calls).toHaveLength(0);
  });
});

describe("WorkspaceViewDialog dismissal", () => {
  it("Escape closes without saving", async () => {
    const user = userEvent.setup();
    seedSession();
    const calls = transport(() => ({ status: 201, body: CREATED }));
    const { onOpenChange } = renderDialog({ mode: "create" });
    await user.type(screen.getByPlaceholderText("Title"), "Never saved");
    await user.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(calls).toHaveLength(0);
  });

  it("Cancel closes without saving", async () => {
    const user = userEvent.setup();
    seedSession();
    const calls = transport(() => ({ status: 201, body: CREATED }));
    const { onOpenChange } = renderDialog({ mode: "create" });
    await user.type(screen.getByPlaceholderText("Title"), "Never saved");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(calls).toHaveLength(0);
  });

  it("reopening resets the form to defaults", async () => {
    const user = userEvent.setup();
    seedSession();
    transport(() => ({ status: 201, body: CREATED }));
    const { rerenderOpen } = renderDialog({ mode: "create" });
    await user.type(screen.getByPlaceholderText("Title"), "Typed");
    await user.type(screen.getByPlaceholderText("Description"), "Typed desc");
    rerenderOpen(false);
    rerenderOpen(true);
    expect(screen.getByPlaceholderText("Title")).toHaveValue("");
    expect(screen.getByPlaceholderText("Description")).toHaveValue("");
  });
});

describe("WorkspaceViewDialog create submit", () => {
  it("creates with the workspace payload, then toasts, closes and navigates", async () => {
    const user = userEvent.setup();
    seedSession();
    const calls = transport(() => ({ status: 201, body: CREATED }));
    const { onOpenChange, queryClient } = renderDialog({ mode: "create" });
    const other = makeView({ id: OTHER_ID, project: null, name: "Older" });
    queryClient.setQueryData(viewKeys.workspaceList("acme"), [other]);
    await user.type(screen.getByPlaceholderText("Title"), "Triage");
    await user.type(screen.getByPlaceholderText("Description"), "spreadsheet view");
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith("View created successfully"));
    expect(calls).toHaveLength(1);
    const post = calls[0]!;
    expect(post.method).toBe("POST");
    expect(post.url).toBe("https://api.test/api/workspaces/acme/views/");
    expect(post.body).toMatchObject({
      name: "Triage",
      description: "spreadsheet view",
      rich_filters: {},
      display_filters: expect.objectContaining({ layout: "spreadsheet", order_by: "-created_at" }),
      display_properties: {},
    });
    expect(post.body).not.toHaveProperty("access");
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(router.navigate).toHaveBeenCalledWith({ to: `/acme/workspace-views/${VIEW_ID}` });
    const { savedViewKeys } = await loadRichFilterBuilder();
    expect(queryClient.getQueryData(savedViewKeys.workspace("acme", VIEW_ID))).toMatchObject({ id: VIEW_ID });
    expect(queryClient.getQueryData(viewKeys.workspaceList("acme"))).toMatchObject([{ id: VIEW_ID }, { id: OTHER_ID }]);
  });

  it("omits an empty description", async () => {
    const user = userEvent.setup();
    seedSession();
    const calls = transport(() => ({ status: 201, body: CREATED }));
    renderDialog({ mode: "create" });
    await user.type(screen.getByPlaceholderText("Title"), "Plain");
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalled());
    expect(calls[0]!.body).toMatchObject({ name: "Plain" });
    expect(calls[0]!.body).not.toHaveProperty("description");
  });

  it("offers the spreadsheet display set without sub-grouping", async () => {
    const user = userEvent.setup();
    seedSession();
    const calls = transport(() => ({ status: 201, body: CREATED }));
    renderDialog({ mode: "create" });
    await user.type(screen.getByPlaceholderText("Title"), "Toggles");
    await user.click(screen.getByTestId("display-options-trigger"));
    expect(await screen.findByRole("region", { name: "Display Properties" })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Sub-group by" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Show sub-work items" }));
    await user.keyboard("{Escape}");
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalled());
    expect(calls[0]!.body).toMatchObject({
      display_filters: expect.objectContaining({ layout: "spreadsheet", sub_issue: false }),
    });
  });

  it("keeps the input on failure so the user can retry", async () => {
    const user = userEvent.setup();
    seedSession();
    let fail = true;
    const calls = transport(() => (fail ? { status: 500, body: {} } : { status: 201, body: CREATED }));
    const { onOpenChange } = renderDialog({ mode: "create" });
    await user.type(screen.getByPlaceholderText("Title"), "Triage");
    await user.type(screen.getByPlaceholderText("Description"), "kept on failure");
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() =>
      expect(notify.notifyError).toHaveBeenCalledWith(
        "View could not be created",
        "Nothing was created — check the details and try again."
      )
    );
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(router.navigate).not.toHaveBeenCalled();
    expect(screen.getByPlaceholderText("Title")).toHaveValue("Triage");
    expect(screen.getByPlaceholderText("Description")).toHaveValue("kept on failure");
    fail = false;
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith("View created successfully"));
    expect(calls).toHaveLength(2);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("names the server detail on failure", async () => {
    const user = userEvent.setup();
    seedSession();
    transport(() => ({ status: 400, body: { detail: "Name taken" } }));
    renderDialog({ mode: "create" });
    await user.type(screen.getByPlaceholderText("Title"), "Triage");
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() => expect(notify.notifyError).toHaveBeenCalledWith("View could not be created", "Name taken"));
  });

  it("falls back when the write throws without a body", async () => {
    const user = userEvent.setup();
    seedSession();
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
      }
      throw new Error("offline");
    }) as never);
    renderDialog({ mode: "create" });
    await user.type(screen.getByPlaceholderText("Title"), "Triage");
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() =>
      expect(notify.notifyError).toHaveBeenCalledWith(
        "View could not be created",
        "Nothing was created — check the details and try again."
      )
    );
  });
});

describe("WorkspaceViewDialog create pending submit", () => {
  function gatedTransport(): { release: () => void } {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
      }
      await gate;
      return { status: 201, url, headers: { get: () => null }, text: async () => JSON.stringify(CREATED) };
    }) as never);
    return { release };
  }

  it("shows a pending label and blocks outside presses while pending", async () => {
    const user = userEvent.setup();
    seedSession();
    const { release } = gatedTransport();
    const { onOpenChange } = renderDialog({ mode: "create" });
    await user.type(screen.getByPlaceholderText("Title"), "Triage");
    await user.click(screen.getByRole("button", { name: "Create View" }));
    const pending = await screen.findByRole("button", { name: "Creating view…" });
    expect(pending).toBeDisabled();
    expect(screen.getByPlaceholderText("Title")).toBeDisabled();
    await user.click(document.body);
    expect(onOpenChange).not.toHaveBeenCalled();
    release();
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith("View created successfully"));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("an Escape while pending closes and the write still settles", async () => {
    const user = userEvent.setup();
    seedSession();
    const { release } = gatedTransport();
    const { onOpenChange } = renderDialog({ mode: "create" });
    await user.type(screen.getByPlaceholderText("Title"), "Triage");
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await screen.findByRole("button", { name: "Creating view…" });
    await user.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledWith(false);
    release();
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith("View created successfully"));
    expect(router.navigate).toHaveBeenCalled();
  });

  it("clears the pending state after a failure", async () => {
    const user = userEvent.setup();
    seedSession();
    transport(() => ({ status: 500, body: {} }));
    renderDialog({ mode: "create" });
    await user.type(screen.getByPlaceholderText("Title"), "Triage");
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() => expect(notify.notifyError).toHaveBeenCalled());
    expect(screen.getByRole("button", { name: "Create View" })).toBeEnabled();
  });
});

describe("WorkspaceViewDialog cloud access", () => {
  function CloudField({ value, onChange, disabled }: ViewAccessFieldProps): React.ReactElement {
    return (
      <div>
        <button type="button" disabled={disabled} aria-pressed={value === 1} onClick={() => onChange(1)}>
          Shared
        </button>
        <button type="button" disabled={disabled} aria-pressed={value === 0} onClick={() => onChange(0)}>
          Owner-only
        </button>
      </div>
    );
  }

  function seedCloud(storeAccess: (input: unknown) => Promise<unknown>): Mock<typeof storeAccess> {
    const store = vi.fn(storeAccess);
    harness.edition = {
      id: "cloud",
      flags: {},
      slots: { viewAccess: { Field: CloudField, storeAccess: store } },
    };
    return store;
  }

  it("stores an owner-only choice through the slot after creating", async () => {
    const user = userEvent.setup();
    seedSession();
    const store = seedCloud(async () => undefined);
    const calls = transport(() => ({ status: 201, body: CREATED }));
    const { onOpenChange } = renderDialog({ mode: "create" });
    expect(screen.getByTestId("view-access-field")).toBeInTheDocument();
    await user.type(screen.getByPlaceholderText("Title"), "Private");
    await user.click(screen.getByRole("button", { name: "Owner-only" }));
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith("View created successfully"));
    expect(calls[0]!.body).not.toHaveProperty("access");
    expect(store).toHaveBeenCalledWith({ workspaceSlug: "acme", projectId: null, viewId: VIEW_ID, access: 0 });
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(router.navigate).toHaveBeenCalledWith({ to: `/acme/workspace-views/${VIEW_ID}` });
  });

  it("skips the store call for the shared default", async () => {
    const user = userEvent.setup();
    seedSession();
    const store = seedCloud(async () => undefined);
    transport(() => ({ status: 201, body: CREATED }));
    renderDialog({ mode: "create" });
    await user.type(screen.getByPlaceholderText("Title"), "Shared");
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalled());
    expect(store).not.toHaveBeenCalled();
  });

  it("keeps the created view when the store call fails", async () => {
    const user = userEvent.setup();
    seedSession();
    seedCloud(async () => {
      throw { body: { detail: "Quota exceeded" } };
    });
    transport(() => ({ status: 201, body: CREATED }));
    const { onOpenChange } = renderDialog({ mode: "create" });
    await user.type(screen.getByPlaceholderText("Title"), "Private");
    await user.click(screen.getByRole("button", { name: "Owner-only" }));
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() =>
      expect(notify.notifyError).toHaveBeenCalledWith(
        "View created, but the access choice could not be saved",
        "Quota exceeded"
      )
    );
    expect(notify.notifySuccess).not.toHaveBeenCalled();
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(router.navigate).toHaveBeenCalled();
  });

  it("shows the stored access read-only on edit", async () => {
    seedSession();
    seedCloud(async () => undefined);
    transport(() => ({ status: 200, body: STORED }));
    renderDialog({ mode: "edit", view: { ...STORED, access: 0 } });
    expect(await screen.findByTestId("workspace-view-access-readonly")).toHaveTextContent("Access: Private");
    expect(screen.queryByTestId("view-access-field")).not.toBeInTheDocument();
  });

  it("shows no access control on edit without the slot", async () => {
    seedSession();
    transport(() => ({ status: 200, body: STORED }));
    renderDialog({ mode: "edit", view: STORED });
    expect(await screen.findByRole("heading", { name: "Update View" })).toBeInTheDocument();
    expect(screen.queryByTestId("view-access-field")).not.toBeInTheDocument();
    expect(screen.queryByTestId("workspace-view-access-readonly")).not.toBeInTheDocument();
  });
});

describe("WorkspaceViewDialog edit", () => {
  it("opens prefilled from the record with its stored filters", async () => {
    seedSession();
    transport(() => ({ status: 200, body: STORED }));
    renderDialog({ mode: "edit", view: STORED });
    expect(await screen.findByRole("heading", { name: "Update View" })).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Title")).toHaveValue("Alpha");
    expect(screen.getByPlaceholderText("Description")).toHaveValue("Before desc");
    expect(screen.getByTestId("display-options-trigger")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Update View" })).toHaveAttribute("type", "submit");
    // The stored expression seeds the temporary builder (no picker reads:
    // priority resolves from static values).
    expect(await screen.findByTestId("rich-filter-row")).toBeInTheDocument();
  });

  it("saves the definition, adopts the echo and refreshes the list row", async () => {
    const user = userEvent.setup();
    seedSession();
    const echo = { ...STORED, name: "Alpha Renamed", description: "After desc" };
    const calls = transport(() => ({ status: 200, body: echo }));
    const { onOpenChange, queryClient } = renderDialog({ mode: "edit", view: STORED });
    const other = makeView({ id: OTHER_ID, project: null, name: "Older" });
    queryClient.setQueryData(viewKeys.workspaceList("acme"), [other, STORED]);
    // The seeded builder must mount (and report its registry) before the
    // save reads it; the keystrokes below yield past its effects.
    await screen.findByTestId("rich-filter-row");
    await user.clear(screen.getByPlaceholderText("Title"));
    await user.type(screen.getByPlaceholderText("Title"), "Alpha Renamed");
    await user.clear(screen.getByPlaceholderText("Description"));
    await user.type(screen.getByPlaceholderText("Description"), "After desc");
    await user.click(screen.getByRole("button", { name: "Update View" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith("View updated successfully"));
    expect(calls).toHaveLength(1);
    const patch = calls[0]!;
    expect(patch.method).toBe("PATCH");
    expect(patch.url).toBe(`https://api.test/api/workspaces/acme/views/${VIEW_ID}/`);
    expect(patch.body).toMatchObject({
      name: "Alpha Renamed",
      description: "After desc",
      rich_filters: { priority__in: "high" },
      display_filters: expect.objectContaining({ layout: "spreadsheet", group_by: "priority" }),
      display_properties: { priority: true },
    });
    expect(patch.body).not.toHaveProperty("access");
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(router.navigate).not.toHaveBeenCalled();
    const { savedViewKeys } = await loadRichFilterBuilder();
    expect(queryClient.getQueryData(savedViewKeys.workspace("acme", VIEW_ID))).toMatchObject({
      name: "Alpha Renamed",
    });
    expect(queryClient.getQueryData(viewKeys.workspaceList("acme"))).toMatchObject([
      { id: OTHER_ID },
      { id: VIEW_ID, name: "Alpha Renamed" },
    ]);
  });

  it("sends an emptied description so clearing persists", async () => {
    const user = userEvent.setup();
    seedSession();
    const calls = transport(() => ({ status: 200, body: { ...STORED, description: "" } }));
    renderDialog({ mode: "edit", view: STORED });
    await user.clear(screen.getByPlaceholderText("Description"));
    await user.click(screen.getByRole("button", { name: "Update View" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalled());
    expect(calls[0]!.body).toMatchObject({ description: "" });
  });

  it("fires an error notice on failure and keeps the dialog open (NEWFRONT-232)", async () => {
    const user = userEvent.setup();
    seedSession();
    transport(() => ({ status: 500, body: {} }));
    const { onOpenChange } = renderDialog({ mode: "edit", view: STORED });
    await user.clear(screen.getByPlaceholderText("Title"));
    await user.type(screen.getByPlaceholderText("Title"), "Alpha Renamed");
    await user.click(screen.getByRole("button", { name: "Update View" }));
    // Intended: the notice fires (the old store swallowed it); the dialog
    // keeps the entered values for retry.
    await waitFor(() =>
      expect(notify.notifyError).toHaveBeenCalledWith(
        "View could not be updated",
        "Nothing was saved — check the details and try again."
      )
    );
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: "Update View" })).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Title")).toHaveValue("Alpha Renamed");
    expect(screen.getByRole("button", { name: "Update View" })).toBeEnabled();
  });

  it("names the server detail on edit failure", async () => {
    const user = userEvent.setup();
    seedSession();
    transport(() => ({ status: 400, body: { detail: "Locked" } }));
    renderDialog({ mode: "edit", view: STORED });
    await user.type(screen.getByPlaceholderText("Title"), " x");
    await user.click(screen.getByRole("button", { name: "Update View" }));
    await waitFor(() => expect(notify.notifyError).toHaveBeenCalledWith("View could not be updated", "Locked"));
  });

  it("shows a pending label while the patch is in flight", async () => {
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
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(STORED) };
    }) as never);
    renderDialog({ mode: "edit", view: STORED });
    await user.type(screen.getByPlaceholderText("Title"), " x");
    await user.click(screen.getByRole("button", { name: "Update View" }));
    expect(await screen.findByRole("button", { name: "Updating view…" })).toBeDisabled();
    release();
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith("View updated successfully"));
  });
});
