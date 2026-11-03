// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const notify = vi.hoisted(() => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));
vi.mock("../../../../shared/shell/index.js", () => notify);
const session = vi.hoisted(() => ({ useMe: vi.fn() }));
vi.mock("../../../../core/session/hooks.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../core/session/hooks.js")>()),
  useMe: session.useMe,
}));

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { useMe } from "../../../../core/session/hooks.js";
import { ViewActions, type ViewActionsProps } from "./ViewActions.js";
import { entityViewContext, viewContextFromRecord, type ViewContext, type ViewSnapshot } from "./views.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const VIEW_ID = "323e4567-e89b-12d3-a456-426614174002";
const OWNER = "423e4567-e89b-12d3-a456-426614174003";
const WORKSPACE = "523e4567-e89b-12d3-a456-426614174004";

const SNAPSHOT: ViewSnapshot = {
  expression: { priority__in: "urgent" },
  displayFilters: { layout: "list" },
  displayProperties: { priority: true },
};

const RECORD = {
  id: VIEW_ID,
  workspace: WORKSPACE,
  project: PROJECT,
  name: "Triage",
  description: "",
  query: {},
  filters: {},
  display_filters: SNAPSHOT.displayFilters,
  display_properties: SNAPSHOT.displayProperties,
  rich_filters: SNAPSHOT.expression,
  access: 0,
  sort_order: 1,
  logo_props: {},
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

function transport(options: { status?: number; body?: unknown } = {}): Call[] {
  const calls: Call[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    const method = init.method ?? "GET";
    calls.push({
      method,
      url,
      body: init.body === undefined ? null : (JSON.parse(init.body) as Record<string, unknown>),
    });
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
    }
    const status = options.status ?? 200;
    return {
      status,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(options.body ?? RECORD),
    };
  }) as never);
  return calls;
}

function savedContext(overrides: { stored?: boolean } = {}): ViewContext {
  const base = viewContextFromRecord(RECORD as never, OWNER, { isDefault: false });
  if (overrides.stored === true) return base;
  // The row's live snapshot mirrors what the view stores, so only a
  // deliberately changed context reads as dirty.
  return {
    ...base,
    storedDisplayFilters: SNAPSHOT.displayFilters,
    storedDisplayProperties: SNAPSHOT.displayProperties,
  };
}

function renderActions(
  mutate: (props: ViewActionsProps) => Partial<ViewActionsProps> = () => ({}),
  me: { id: string } | null = { id: OWNER }
): {
  calls: Call[];
  onClearAll: ReturnType<typeof vi.fn>;
  onCreated: ReturnType<typeof vi.fn>;
  onUpdated: ReturnType<typeof vi.fn>;
} {
  const calls = transport();
  vi.mocked(useMe).mockReturnValue({ data: me } as never);
  const onClearAll = vi.fn();
  const onCreated = vi.fn();
  const onUpdated = vi.fn();
  const base: ViewActionsProps = {
    workspaceSlug: "acme",
    projectId: PROJECT,
    snapshot: SNAPSHOT,
    context: entityViewContext(),
    canEdit: true,
    viewsEnabled: true,
    meId: OWNER,
    onClearAll,
    hasConditions: true,
    onCreated,
    onUpdated,
  };
  render(<ViewActions {...base} {...mutate(base)} />);
  return { calls, onClearAll, onCreated, onUpdated };
}

/**
 * Open the create dialog and type a name once its focus trap has settled.
 * Base UI moves focus asynchronously after mount; typing before that lands
 * on a focus guard instead of the name field.
 */
async function openNamedDialog(
  user: ReturnType<typeof userEvent.setup>,
  name: string,
  trigger = "Save view"
): Promise<void> {
  await user.click(screen.getByRole("button", { name: trigger }));
  await screen.findByRole("dialog");
  // The dialog focuses its close control first; type into the name field.
  await user.click(screen.getByLabelText("View name"));
  await user.type(screen.getByLabelText("View name"), name);
  await waitFor(() => expect(screen.getByLabelText("View name")).toHaveValue(name));
}

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
});

describe("ViewActions", () => {
  it("renders nothing for a bare list with no conditions and no save offer", () => {
    renderActions((props) => ({
      ...props,
      view: undefined,
      context: entityViewContext(),
      hasConditions: false,
      viewsEnabled: false,
    }));
    expect(screen.queryByTestId("rich-view-actions")).not.toBeInTheDocument();
  });

  it("clears every condition from the row", async () => {
    const user = userEvent.setup();
    const { onClearAll } = renderActions();
    await user.click(screen.getByTestId("rich-clear-all"));
    expect(onClearAll).toHaveBeenCalledTimes(1);
  });

  it("keeps only the row actions when there is nothing to clear", () => {
    renderActions((props) => ({ ...props, hasConditions: false }));
    expect(screen.queryByTestId("rich-clear-all")).not.toBeInTheDocument();
    // There is no expression to save, so neither Save entry offers.
    expect(screen.queryByTestId("rich-save-view")).not.toBeInTheDocument();
  });

  it("offers only Update view once a saved view is cleared", async () => {
    const user = userEvent.setup();
    const { calls, onClearAll } = renderActions((props) => ({
      ...props,
      context: savedContext(),
      // The live row is now empty; the view still stores the expression.
      snapshot: { ...SNAPSHOT, expression: {} },
      hasConditions: false,
    }));
    expect(screen.queryByTestId("rich-clear-all")).not.toBeInTheDocument();
    expect(screen.queryByTestId("rich-save-view")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Update view" }));
    await waitFor(() => expect(calls.some((call) => call.method === "PATCH")).toBe(true));
    expect(calls.find((call) => call.method === "PATCH")?.body).toMatchObject({ rich_filters: {} });
    expect(onClearAll).not.toHaveBeenCalled();
  });

  it("creates a project view from the dialog", async () => {
    const user = userEvent.setup();
    const { calls, onCreated } = renderActions();
    await user.click(screen.getByTestId("rich-save-view"));
    await screen.findByRole("dialog");
    const create = screen.getByRole("button", { name: "Create View" });
    expect(create).toBeDisabled();
    await user.type(screen.getByLabelText("View name"), "  My view  ");
    await user.click(create);
    await waitFor(() => expect(onCreated).toHaveBeenCalledTimes(1));
    const post = calls.find((call) => call.method === "POST");
    expect(post?.url).toContain(`/api/workspaces/acme/projects/${PROJECT}/views/`);
    expect(post?.body).toEqual({
      name: "My view",
      rich_filters: SNAPSHOT.expression,
      display_filters: SNAPSHOT.displayFilters,
      display_properties: SNAPSHOT.displayProperties,
    });
    expect(notify.notifySuccess).toHaveBeenCalledWith("View created successfully");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("closes the dialog from Cancel without creating anything", async () => {
    const user = userEvent.setup();
    const { calls, onCreated } = renderActions();
    await user.click(screen.getByTestId("rich-save-view"));
    await screen.findByRole("dialog");
    await new Promise((resolve) => setTimeout(resolve, 50));
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(calls.some((call) => call.method === "POST")).toBe(false);
    expect(onCreated).not.toHaveBeenCalled();
  });

  it("dismisses the dialog through its own open-state handler", async () => {
    const user = userEvent.setup();
    renderActions();
    await user.click(screen.getByTestId("rich-save-view"));
    await screen.findByRole("dialog");
    await new Promise((resolve) => setTimeout(resolve, 50));
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("ignores a bare error object with no body", async () => {
    const user = userEvent.setup();
    renderActions();
    resetAppClient();
    createAppClient("https://api.test", (() => {
      throw { message: "not an ApiError" };
    }) as never);
    await openNamedDialog(user, "Triage");
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() =>
      expect(notify.notifyError).toHaveBeenCalledWith(
        "Couldn't save the view",
        "Nothing was saved — check your connection and try again."
      )
    );
  });

  it("reports a create failure with the fixed copy for a non-API error", async () => {
    const user = userEvent.setup();
    renderActions();
    resetAppClient();
    createAppClient("https://api.test", (() => {
      throw new Error("offline");
    }) as never);
    await user.click(screen.getByTestId("rich-save-view"));
    await user.type(await screen.findByLabelText("View name"), "Triage");
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() =>
      expect(notify.notifyError).toHaveBeenCalledWith(
        "Couldn't save the view",
        "Nothing was saved — check your connection and try again."
      )
    );
  });

  it("creates a workspace view when the row edits a workspace view", async () => {
    const user = userEvent.setup();
    // Save as needs a dirty view; the changed expression offers it.
    const context = {
      ...savedContext(),
      viewScope: "workspace" as const,
      storedExpression: { priority__in: "low" },
    };
    const { calls } = renderActions((props) => ({ ...props, context }));
    await openNamedDialog(user, "Copy", "Save as");
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() => expect(calls.some((call) => call.method === "POST")).toBe(true));
    expect(calls.find((call) => call.method === "POST")?.url).toContain("/api/workspaces/acme/views/");
  });

  it("reports a create failure and keeps the dialog open", async () => {
    const user = userEvent.setup();
    renderActions();
    resetAppClient();
    transport({ status: 400, body: { detail: "Name already taken" } });
    await openNamedDialog(user, "Triage");
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() =>
      expect(notify.notifyError).toHaveBeenCalledWith("Couldn't save the view", "Name already taken")
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("hides save and update without the views feature or edit rights", () => {
    renderActions((props) => ({ ...props, viewsEnabled: false }));
    expect(screen.queryByTestId("rich-save-view")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Update view" })).not.toBeInTheDocument();
    expect(screen.getByTestId("rich-clear-all")).toBeInTheDocument();
  });

  it("offers neither Save as nor Update view while the view is clean", () => {
    renderActions((props) => ({ ...props, context: savedContext() }));
    expect(screen.queryByTestId("rich-save-view")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Update view" })).not.toBeInTheDocument();
  });

  it("offers Save as next to Update view once the view is dirty", () => {
    const changed = { ...savedContext(), storedExpression: { priority__in: "low" } };
    renderActions((props) => ({ ...props, context: changed }));
    expect(screen.getByRole("button", { name: "Save as" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Update view" })).toBeInTheDocument();
  });

  it("offers Save as on a layout-only change with no live conditions", () => {
    // The stored expression rides the fork even though the row is empty.
    const layoutDirty = { ...savedContext(), storedDisplayFilters: { layout: "kanban" } };
    renderActions((props) => ({ ...props, context: layoutDirty, hasConditions: false }));
    expect(screen.getByRole("button", { name: "Save as" })).toBeInTheDocument();
  });

  it("delegates the save press to the host dialog instead of the minimal form", async () => {
    const user = userEvent.setup();
    const onSavePress = vi.fn();
    const changed = { ...savedContext(), storedExpression: { priority__in: "low" } };
    const { calls } = renderActions((props) => ({ ...props, context: changed, onSavePress }));
    await user.click(screen.getByRole("button", { name: "Save as" }));
    expect(onSavePress).toHaveBeenCalledTimes(1);
    expect(onSavePress).toHaveBeenCalledWith(SNAPSHOT);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(calls.some((call) => call.method === "POST")).toBe(false);
  });

  it("delegates Save view on a bare list the same way", async () => {
    const user = userEvent.setup();
    const onSavePress = vi.fn();
    renderActions((props) => ({ ...props, onSavePress }));
    await user.click(screen.getByRole("button", { name: "Save view" }));
    expect(onSavePress).toHaveBeenCalledWith(SNAPSHOT);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("falls back to my own id when the caller passes none", async () => {
    const user = userEvent.setup();
    const changed = { ...savedContext(), storedExpression: { priority__in: "low" } };
    renderActions((props) => ({ ...props, context: changed, meId: undefined as never }), { id: OWNER });
    await user.click(await screen.findByRole("button", { name: "Update view" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith("View updated successfully"));
  });

  it("updates a changed, owned view through the project endpoint", async () => {
    const user = userEvent.setup();
    const changed = { ...savedContext(), storedExpression: { priority__in: "low" } };
    const { calls, onUpdated } = renderActions((props) => ({ ...props, context: changed }));
    await user.click(screen.getByRole("button", { name: "Update view" }));
    await waitFor(() => expect(onUpdated).toHaveBeenCalledTimes(1));
    const patch = calls.find((call) => call.method === "PATCH");
    expect(patch?.url).toContain(`/api/workspaces/acme/projects/${PROJECT}/views/${VIEW_ID}/`);
    expect(patch?.body).toEqual({
      rich_filters: SNAPSHOT.expression,
      display_filters: SNAPSHOT.displayFilters,
      display_properties: SNAPSHOT.displayProperties,
    });
    expect(notify.notifySuccess).toHaveBeenCalledWith("View updated successfully");
  });

  it("picks the project collection for a bare list even in a workspace context", async () => {
    const user = userEvent.setup();
    const context = { ...entityViewContext(), viewScope: "workspace" as const };
    const { calls } = renderActions((props) => ({ ...props, context }));
    await openNamedDialog(user, "Mine");
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() => expect(calls.some((call) => call.method === "POST")).toBe(true));
    expect(calls.find((call) => call.method === "POST")?.url).toContain(
      `/api/workspaces/acme/projects/${PROJECT}/views/`
    );
  });

  it("updates a workspace view through the workspace endpoint", async () => {
    const user = userEvent.setup();
    const changed = {
      ...savedContext(),
      viewScope: "workspace" as const,
      storedExpression: { priority__in: "low" },
    };
    const { calls } = renderActions((props) => ({ ...props, context: changed }));
    await user.click(screen.getByRole("button", { name: "Update view" }));
    await waitFor(() => expect(calls.some((call) => call.method === "PATCH")).toBe(true));
    expect(calls.find((call) => call.method === "PATCH")?.url).toContain(`/api/workspaces/acme/views/${VIEW_ID}/`);
  });

  it("reports an update failure with the fixed copy when the body has no detail", async () => {
    const user = userEvent.setup();
    const changed = { ...savedContext(), storedExpression: { priority__in: "low" } };
    renderActions((props) => ({ ...props, context: changed }));
    resetAppClient();
    transport({ status: 500, body: { error: "boom" } });
    await user.click(screen.getByRole("button", { name: "Update view" }));
    await waitFor(() =>
      expect(notify.notifyError).toHaveBeenCalledWith(
        "Couldn't update the view",
        "The view is unchanged — check your connection and try again."
      )
    );
  });

  it("never offers update for another owner, a locked view or a default", () => {
    const changed = { ...savedContext(), storedExpression: { priority__in: "low" } };
    for (const [context, meId] of [
      [changed, "someone-else"],
      [{ ...changed, locked: true }, OWNER],
      [{ ...changed, isDefault: true }, OWNER],
    ] as const) {
      const { unmount } = render(
        <ViewActions
          workspaceSlug="acme"
          projectId={PROJECT}
          snapshot={SNAPSHOT}
          context={context}
          canEdit={true}
          viewsEnabled={true}
          meId={meId}
          onClearAll={vi.fn()}
          hasConditions={true}
        />
      );
      expect(screen.queryByRole("button", { name: "Update view" })).not.toBeInTheDocument();
      unmount();
    }
  });
});
