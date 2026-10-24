// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Delete confirmation (parity/specs/modules.md §7.5, MOD-025): the copy
// names the module and warns it is permanent; confirming deletes, shows a
// success notice and reports it to the host; a 403 speaks with the pinned
// permission title and leaves the module in place; any other refusal shows
// the server's sentence. Cancel never writes.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const notify = vi.hoisted(() => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));
vi.mock("../../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../shared/shell/index.js")>()),
  notifyError: notify.notifyError,
  notifySuccess: notify.notifySuccess,
}));

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { moduleKeys } from "../../api/keys.js";
import { jsonResponse, MODULES_MODULE_ID, MODULES_PROJECT_ID } from "../../testing.js";
import { ModuleDeleteDialog } from "./ModuleDeleteDialog.js";

const PATH = `/api/workspaces/acme/projects/${MODULES_PROJECT_ID}/modules/${MODULES_MODULE_ID}/`;

interface Harness {
  status: number;
  body: unknown;
  calls: string[];
}

let harness: Harness;

beforeEach(() => {
  harness = { status: 204, body: null, calls: [] };
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (init.method === "DELETE" && url.endsWith(PATH)) {
      harness.calls.push(url);
      if (harness.status === 204) return jsonResponse(url, 204, null);
      return jsonResponse(url, harness.status, harness.body);
    }
    throw new Error(`unexpected request: ${init.method ?? "GET"} ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  notify.notifyError.mockReset();
  notify.notifySuccess.mockReset();
});

function renderDialog(overrides: Partial<React.ComponentProps<typeof ModuleDeleteDialog>> = {}) {
  const onOpenChange = vi.fn();
  const onDeleted = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const ui = (
    <QueryClientProvider client={queryClient}>
      <ModuleDeleteDialog
        module={{ id: MODULES_MODULE_ID, name: "Delivery" }}
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        onOpenChange={onOpenChange}
        onDeleted={onDeleted}
        {...overrides}
      />
    </QueryClientProvider>
  );
  const view = render(ui);
  return { onOpenChange, onDeleted, queryClient, ...view };
}

describe("ModuleDeleteDialog", () => {
  it("names the module and states that the deletion is permanent", () => {
    renderDialog();
    expect(screen.getByRole("heading", { name: "Delete module" })).toBeInTheDocument();
    const body = screen.getByTestId("module-delete-body");
    expect(body).toHaveTextContent("Delivery");
    expect(body).toHaveTextContent(/permanently removed/);
    expect(body).toHaveTextContent(/cannot be undone/);
  });

  it("stays closed while no module is targeted", () => {
    renderDialog({ module: null });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("deletes on confirm, notices the removal and reports the module", async () => {
    const { onOpenChange, onDeleted, queryClient } = renderDialog();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    expect(harness.calls).toEqual([`https://api.test${PATH}`]);
    expect(notify.notifySuccess).toHaveBeenCalledWith("Success!", expect.stringContaining("Delivery"));
    expect(onDeleted).toHaveBeenCalledWith({ id: MODULES_MODULE_ID, name: "Delivery" });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: moduleKeys.list("acme", MODULES_PROJECT_ID) });
    expect(notify.notifyError).not.toHaveBeenCalled();
  });

  it("cancels without writing", () => {
    const { onOpenChange, onDeleted } = renderDialog();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(onDeleted).not.toHaveBeenCalled();
    expect(harness.calls).toHaveLength(0);
  });

  it("shows the pinned permission title on a 403 and leaves the module alone", async () => {
    harness.status = 403;
    harness.body = { detail: "You cannot delete this module." };
    const { onOpenChange, onDeleted } = renderDialog();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    expect(notify.notifyError).toHaveBeenCalledWith(
      "You don't have permission to perform this action.",
      "You cannot delete this module."
    );
    expect(notify.notifySuccess).not.toHaveBeenCalled();
    expect(onDeleted).not.toHaveBeenCalled();
  });

  it("reports any other refusal and closes without claiming success", async () => {
    harness.status = 500;
    harness.body = { detail: "Something broke." };
    const { onOpenChange, onDeleted } = renderDialog();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    expect(notify.notifyError).toHaveBeenCalledWith("Error!", "Something broke.");
    expect(notify.notifySuccess).not.toHaveBeenCalled();
    expect(onDeleted).not.toHaveBeenCalled();
  });

  it("keeps the confirmation up while the delete flies", async () => {
    let release: () => void = () => undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const base = harness;
    createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
      if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
      if (init.method === "DELETE") {
        base.calls.push(url);
        await gate;
        return jsonResponse(url, 204, null);
      }
      throw new Error(`unexpected request: ${url}`);
    }) as never);
    const { onOpenChange } = renderDialog();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Delete" })).toBeDisabled());
    expect(onOpenChange).not.toHaveBeenCalled();
    release();
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
  });
});
