// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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
import type { ViewAccessFieldProps } from "../../../core/edition/index.js";
import { savedViewKeys } from "../../issues/index.js";
import { CreateViewDialog } from "../index.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const VIEW_ID = "323e4567-e89b-12d3-a456-426614174002";
const OWNER = "423e4567-e89b-12d3-a456-426614174003";
const WORKSPACE_ID = "523e4567-e89b-12d3-a456-426614174004";

const RECORD = {
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
};

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

function renderDialog(options: { open?: boolean; onOpenChange?: (open: boolean) => void } = {}): {
  onOpenChange: ReturnType<typeof vi.fn>;
  queryClient: QueryClient;
  rerenderOpen: (open: boolean) => void;
} {
  const onOpenChange = (options.onOpenChange ?? vi.fn()) as ReturnType<typeof vi.fn>;
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const ui = (open: boolean): React.ReactElement => (
    <QueryClientProvider client={queryClient}>
      <CreateViewDialog
        open={open}
        onOpenChange={onOpenChange}
        workspaceSlug="acme"
        projectId={PROJECT}
        hideCycle={false}
        hideModule={false}
      />
    </QueryClientProvider>
  );
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

describe("CreateViewDialog blank form", () => {
  it("opens with defaults, an expanded filter builder and no access selector", async () => {
    seedSession();
    transport(() => ({ status: 201, body: RECORD }));
    renderDialog();
    expect(screen.getByRole("heading", { name: "Create View" })).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Title")).toHaveValue("");
    expect(screen.getByPlaceholderText("Description")).toHaveValue("");
    expect(screen.getByRole("button", { name: "List" })).toBeInTheDocument();
    expect(screen.getByTestId("display-options-trigger")).toBeInTheDocument();
    // The filter section arrives with its lazily loaded chunk.
    expect(await screen.findByTestId("rich-filter-add")).toBeInTheDocument();
    expect(screen.queryByTestId("view-access-field")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create View" })).toHaveAttribute("type", "submit");
  });

  it("focuses the title on open", () => {
    seedSession();
    transport(() => ({ status: 201, body: RECORD }));
    renderDialog();
    expect(screen.getByPlaceholderText("Title")).toHaveFocus();
  });
});

describe("CreateViewDialog validation", () => {
  it("requires a title", async () => {
    const user = userEvent.setup();
    seedSession();
    const calls = transport(() => ({ status: 201, body: RECORD }));
    const { onOpenChange } = renderDialog();
    await user.click(screen.getByRole("button", { name: "Create View" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Title is required");
    expect(calls).toHaveLength(0);
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("caps the title at 255 characters", async () => {
    const user = userEvent.setup();
    seedSession();
    const calls = transport(() => ({ status: 201, body: RECORD }));
    renderDialog();
    await user.type(screen.getByPlaceholderText("Title"), "x".repeat(256));
    await user.click(screen.getByRole("button", { name: "Create View" }));
    expect(screen.getByRole("alert").textContent).toContain("255");
    expect(calls).toHaveLength(0);
  });

  it("clears the error while retyping", async () => {
    const user = userEvent.setup();
    seedSession();
    transport(() => ({ status: 201, body: RECORD }));
    renderDialog();
    await user.click(screen.getByRole("button", { name: "Create View" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Title is required");
    await user.type(screen.getByPlaceholderText("Title"), "Triage");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});

describe("CreateViewDialog dismissal", () => {
  it("Escape closes without saving", async () => {
    const user = userEvent.setup();
    seedSession();
    const calls = transport(() => ({ status: 201, body: RECORD }));
    const { onOpenChange } = renderDialog();
    await user.type(screen.getByPlaceholderText("Title"), "Never saved");
    await user.type(screen.getByPlaceholderText("Description"), "discarded");
    await user.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(calls).toHaveLength(0);
  });

  it("Cancel closes without saving", async () => {
    const user = userEvent.setup();
    seedSession();
    const calls = transport(() => ({ status: 201, body: RECORD }));
    const { onOpenChange } = renderDialog();
    await user.type(screen.getByPlaceholderText("Title"), "Never saved");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(calls).toHaveLength(0);
  });

  it("reopening resets the form to defaults", async () => {
    const user = userEvent.setup();
    seedSession();
    transport(() => ({ status: 201, body: RECORD }));
    const { rerenderOpen } = renderDialog();
    await user.type(screen.getByPlaceholderText("Title"), "Typed");
    await user.click(screen.getByRole("button", { name: "List" }));
    await user.click(screen.getByRole("option", { name: "Board" }));
    expect(screen.getByRole("button", { name: "Board" })).toBeInTheDocument();
    rerenderOpen(false);
    rerenderOpen(true);
    expect(screen.getByPlaceholderText("Title")).toHaveValue("");
    expect(screen.getByPlaceholderText("Description")).toHaveValue("");
    expect(screen.getByRole("button", { name: "List" })).toBeInTheDocument();
  });
});

describe("CreateViewDialog submit", () => {
  async function fillBoardWithGlyph(user: ReturnType<typeof userEvent.setup>): Promise<void> {
    await user.type(screen.getByPlaceholderText("Title"), "Triage");
    await user.type(screen.getByPlaceholderText("Description"), "board with icon");
    await user.click(screen.getByRole("button", { name: "List" }));
    await user.click(screen.getByRole("option", { name: "Board" }));
    await user.click(screen.getByTestId("view-icon-picker"));
    await user.click(screen.getByRole("tab", { name: "Icon" }));
    await user.click(within(screen.getByTestId("view-icon-glyphs")).getByRole("button", { name: "View icon Flag" }));
  }

  it("creates with the full payload, then toasts, closes and navigates", async () => {
    const user = userEvent.setup();
    seedSession();
    const calls = transport(() => ({ status: 201, body: RECORD }));
    const { onOpenChange, queryClient } = renderDialog();
    await fillBoardWithGlyph(user);
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith("View created successfully"));
    expect(calls).toHaveLength(1);
    const post = calls[0]!;
    expect(post.method).toBe("POST");
    expect(post.url).toContain(`/api/workspaces/acme/projects/${PROJECT}/views/`);
    expect(post.body).toMatchObject({
      name: "Triage",
      description: "board with icon",
      rich_filters: {},
      display_filters: expect.objectContaining({ layout: "kanban", group_by: "state" }),
      display_properties: {},
      logo_props: { in_use: "icon", icon: { name: "Flag", color: "#5b5bd6" } },
    });
    expect(post.body).not.toHaveProperty("access");
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(router.navigate).toHaveBeenCalledWith({
      to: "/$ws/projects/$projectId/views/$viewId",
      params: { ws: "acme", projectId: PROJECT, viewId: VIEW_ID },
    });
    expect(queryClient.getQueryData(savedViewKeys.project("acme", PROJECT, VIEW_ID))).toMatchObject({ id: VIEW_ID });
  });

  it("sends the List defaults untouched", async () => {
    const user = userEvent.setup();
    seedSession();
    const calls = transport(() => ({ status: 201, body: RECORD }));
    renderDialog();
    await user.type(screen.getByPlaceholderText("Title"), "Plain");
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalled());
    expect(calls[0]!.body).toMatchObject({
      name: "Plain",
      display_filters: expect.objectContaining({ layout: "list", group_by: "state" }),
    });
    expect(calls[0]!.body).not.toHaveProperty("description");
    expect(calls[0]!.body).not.toHaveProperty("logo_props");
  });

  it("stores an emoji pick", async () => {
    const user = userEvent.setup();
    seedSession();
    const calls = transport(() => ({ status: 201, body: RECORD }));
    renderDialog();
    await user.type(screen.getByPlaceholderText("Title"), "Emoji");
    await user.click(screen.getByTestId("view-icon-picker"));
    await user.click(within(screen.getByTestId("view-icon-emojis")).getByRole("button", { name: "Emoji 🚀" }));
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalled());
    expect(calls[0]!.body).toMatchObject({ logo_props: { in_use: "emoji", emoji: { value: "🚀" } } });
  });

  it("carries display edits into the payload", async () => {
    const user = userEvent.setup();
    seedSession();
    const calls = transport(() => ({ status: 201, body: RECORD }));
    renderDialog();
    await user.type(screen.getByPlaceholderText("Title"), "Grouped");
    await user.click(screen.getByTestId("display-options-trigger"));
    // Driver contract (VIEW-013): the panel names its sections.
    expect(await screen.findByRole("region", { name: "Display Properties" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Group by" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Show empty groups" }));
    await user.keyboard("{Escape}");
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalled());
    expect(calls[0]!.body).toMatchObject({
      display_filters: expect.objectContaining({ show_empty_groups: true }),
    });
  });

  it("keeps the input on failure so the user can retry", async () => {
    const user = userEvent.setup();
    seedSession();
    let fail = true;
    const calls = transport(() => (fail ? { status: 500, body: {} } : { status: 201, body: RECORD }));
    const { onOpenChange } = renderDialog();
    await user.type(screen.getByPlaceholderText("Title"), "Triage");
    await user.type(screen.getByPlaceholderText("Description"), "kept on failure");
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() =>
      expect(notify.notifyError).toHaveBeenCalledWith(
        "Failed to create view",
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
    renderDialog();
    await user.type(screen.getByPlaceholderText("Title"), "Triage");
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() => expect(notify.notifyError).toHaveBeenCalledWith("Failed to create view", "Name taken"));
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
    renderDialog();
    await user.type(screen.getByPlaceholderText("Title"), "Triage");
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() =>
      expect(notify.notifyError).toHaveBeenCalledWith(
        "Failed to create view",
        "Nothing was created — check the details and try again."
      )
    );
  });
});

describe("CreateViewDialog pending submit", () => {
  function gatedTransport(): { calls: Call[]; release: () => void } {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const calls: Call[] = [];
    createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
      }
      calls.push({
        method: init.method ?? "GET",
        url,
        body: init.body === undefined ? null : (JSON.parse(init.body) as Record<string, unknown>),
      });
      await gate;
      return { status: 201, url, headers: { get: () => null }, text: async () => JSON.stringify(RECORD) };
    }) as never);
    return { calls, release };
  }

  it("shows a pending label and blocks outside presses while pending", async () => {
    const user = userEvent.setup();
    seedSession();
    const { release } = gatedTransport();
    const { onOpenChange } = renderDialog();
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
    const { onOpenChange } = renderDialog();
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
    renderDialog();
    await user.type(screen.getByPlaceholderText("Title"), "Triage");
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() => expect(notify.notifyError).toHaveBeenCalled());
    expect(screen.getByRole("button", { name: "Create View" })).toBeEnabled();
  });
});

describe("CreateViewDialog cloud access", () => {
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

  function seedCloud(storeAccess: (input: unknown) => Promise<unknown>): ReturnType<typeof vi.fn> {
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
    const calls = transport(() => ({ status: 201, body: RECORD }));
    const { onOpenChange } = renderDialog();
    expect(screen.getByTestId("view-access-field")).toBeInTheDocument();
    await user.type(screen.getByPlaceholderText("Title"), "Private");
    await user.click(screen.getByRole("button", { name: "Owner-only" }));
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith("View created successfully"));
    expect(calls[0]!.body).not.toHaveProperty("access");
    expect(store).toHaveBeenCalledWith({ workspaceSlug: "acme", projectId: PROJECT, viewId: VIEW_ID, access: 0 });
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(router.navigate).toHaveBeenCalledWith({
      to: "/$ws/projects/$projectId/views/$viewId",
      params: { ws: "acme", projectId: PROJECT, viewId: VIEW_ID },
    });
  });

  it("skips the store call for the shared default", async () => {
    const user = userEvent.setup();
    seedSession();
    const store = seedCloud(async () => undefined);
    transport(() => ({ status: 201, body: RECORD }));
    renderDialog();
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
    transport(() => ({ status: 201, body: RECORD }));
    const { onOpenChange } = renderDialog();
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
});
