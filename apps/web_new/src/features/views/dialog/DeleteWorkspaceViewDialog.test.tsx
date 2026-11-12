// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const notify = vi.hoisted(() => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));
vi.mock("../../../shared/shell/index.js", () => notify);

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { viewKeys } from "../api/keys.js";
import { DeleteWorkspaceViewDialog } from "../index.js";
import { makeView } from "../testing.js";

const VIEW_ID = "323e4567-e89b-12d3-a456-426614174002";
const OTHER_ID = "623e4567-e89b-12d3-a456-426614174005";

const VIEW = makeView({ id: VIEW_ID, project: null, name: "Alpha" });

interface Call {
  method: string;
  url: string;
}

function transport(respond: (call: Call) => { status: number; body: unknown }): Call[] {
  const calls: Call[] = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
    }
    const call: Call = { method: init.method ?? "GET", url };
    calls.push(call);
    const { status, body } = respond(call);
    return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
  }) as never);
  return calls;
}

function renderDialog(options: { onDeleted?: (viewId: string) => void } = {}): {
  onOpenChange: ReturnType<typeof vi.fn>;
  onDeleted: ReturnType<typeof vi.fn>;
  queryClient: QueryClient;
} {
  const onOpenChange = vi.fn();
  const onDeleted = vi.fn(options.onDeleted);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <DeleteWorkspaceViewDialog
        open={true}
        onOpenChange={onOpenChange}
        workspaceSlug="acme"
        view={VIEW}
        onDeleted={options.onDeleted === undefined ? undefined : onDeleted}
      />
    </QueryClientProvider>
  );
  return { onOpenChange, onDeleted, queryClient };
}

afterEach(() => {
  resetAppClient();
  notify.notifyError.mockClear();
  notify.notifySuccess.mockClear();
});

describe("DeleteWorkspaceViewDialog", () => {
  it("confirms the permanent loss and cancels without deleting", async () => {
    const user = userEvent.setup();
    const calls = transport(() => ({ status: 204, body: null }));
    const { onOpenChange } = renderDialog();
    expect(screen.getByRole("heading", { name: "Are you sure you want to delete this view?" })).toBeInTheDocument();
    expect(screen.getByText(/permanently deleted/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(calls).toHaveLength(0);
    expect(notify.notifySuccess).not.toHaveBeenCalled();
  });

  it("Escape closes without deleting", async () => {
    const user = userEvent.setup();
    const calls = transport(() => ({ status: 204, body: null }));
    const { onOpenChange } = renderDialog();
    await user.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(calls).toHaveLength(0);
  });

  it("deletes, drops the row from the list cache and reports back", async () => {
    const user = userEvent.setup();
    const calls = transport((call) =>
      call.method === "GET" ? { status: 200, body: [] } : { status: 204, body: null }
    );
    const other = makeView({ id: OTHER_ID, project: null, name: "Older" });
    const { onOpenChange, onDeleted, queryClient } = renderDialog({ onDeleted: () => {} });
    queryClient.setQueryData(viewKeys.workspaceList("acme"), [other, VIEW]);
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith("View deleted successfully"));
    const remove = calls.find((call) => call.method === "DELETE")!;
    expect(remove.url).toBe(`https://api.test/api/workspaces/acme/views/${VIEW_ID}/`);
    expect(queryClient.getQueryData(viewKeys.workspaceList("acme"))).toMatchObject([{ id: OTHER_ID }]);
    expect(onDeleted).toHaveBeenCalledWith(VIEW_ID);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("deletes without a report callback and without a cached list", async () => {
    const user = userEvent.setup();
    transport(() => ({ status: 204, body: null }));
    const { onOpenChange, queryClient } = renderDialog();
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith("View deleted successfully"));
    expect(queryClient.getQueryData(viewKeys.workspaceList("acme"))).toBeUndefined();
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("keeps the row and the dialog on failure", async () => {
    const user = userEvent.setup();
    transport(() => ({ status: 500, body: {} }));
    const { onOpenChange, onDeleted, queryClient } = renderDialog({ onDeleted: () => {} });
    queryClient.setQueryData(viewKeys.workspaceList("acme"), [VIEW]);
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(notify.notifyError).toHaveBeenCalledWith(
        "Failed to delete view",
        "The view was kept — check the details and try again."
      )
    );
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(onDeleted).not.toHaveBeenCalled();
    expect(queryClient.getQueryData(viewKeys.workspaceList("acme"))).toMatchObject([{ id: VIEW_ID }]);
    expect(screen.getByRole("button", { name: "Delete" })).toBeEnabled();
  });

  it("names the server detail on failure", async () => {
    const user = userEvent.setup();
    transport(() => ({ status: 403, body: { detail: "Not the owner" } }));
    renderDialog();
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(notify.notifyError).toHaveBeenCalledWith("Failed to delete view", "Not the owner"));
  });

  it("shows a pending label and swallows outside presses while deleting", async () => {
    const user = userEvent.setup();
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    createAppClient("https://api.test", (async (url: string) => {
      if (url.endsWith("/auth/get-csrf-token/")) {
        return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
      }
      await gate;
      return { status: 204, url, headers: { get: () => null }, text: async () => "null" };
    }) as never);
    const { onOpenChange } = renderDialog();
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(await screen.findByRole("button", { name: "Deleting…" })).toBeDisabled();
    await user.click(document.body);
    expect(onOpenChange).not.toHaveBeenCalled();
    await user.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledWith(false);
    release();
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith("View deleted successfully"));
  });
});
