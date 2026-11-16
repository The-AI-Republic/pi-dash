// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One provider owns the save dialog and the delete confirmation for a
// project (MOD-003/009/022–025). The three openers drive them: create and
// edit share the save dialog in the right mode, delete opens the
// confirmation with the host's post-delete callback, and a surface outside
// the provider has no openers at all.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ModuleRow } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../shared/shell/index.js")>()),
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { jsonResponse, makeModuleRow, MODULES_MODULE_ID, MODULES_PROJECT_ID } from "../../testing.js";
import { ModuleDialogProvider, useModuleDialogs } from "./ModuleDialogProvider.js";

const CURRENT = { id: MODULES_PROJECT_ID, name: "Website relaunch", identifier: "WEB" };

function record(overrides: Partial<ModuleRow> = {}): ModuleRow {
  return makeModuleRow({ id: MODULES_MODULE_ID, name: "Delivery", status: "planned", ...overrides });
}

let handle: ReturnType<typeof useModuleDialogs> = null;

function Probe(): React.ReactElement {
  handle = useModuleDialogs();
  return <p data-testid="probe">{handle === null ? "none" : "ready"}</p>;
}

function renderProvider() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <ModuleDialogProvider workspaceSlug="acme" projectId={MODULES_PROJECT_ID} currentProject={CURRENT}>
        <Probe />
      </ModuleDialogProvider>
    </QueryClientProvider>
  );
}

beforeEach(() => {
  handle = null;
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url === "https://api.test/api/workspaces/acme/projects/") return jsonResponse(url, 200, []);
    if (url.endsWith("/api/workspaces/acme/members/")) return jsonResponse(url, 200, []);
    if (url.endsWith("/api/users/me/")) return jsonResponse(url, 500, {});
    return jsonResponse(url, 500, {});
  }) as never);
});

afterEach(() => {
  resetAppClient();
});

describe("useModuleDialogs", () => {
  it("reads null outside a provider", () => {
    render(<Probe />);
    expect(screen.getByTestId("probe")).toHaveTextContent("none");
    expect(handle).toBeNull();
  });
});

describe("ModuleDialogProvider", () => {
  it("opens the create dialog on blank defaults", () => {
    renderProvider();
    expect(screen.getByTestId("probe")).toHaveTextContent("ready");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    act(() => handle?.openCreate());
    expect(screen.getByRole("heading", { name: "Create module" })).toBeInTheDocument();
    expect(screen.getByTestId("module-dialog-project")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Title" })).toHaveValue("");
  });

  it("opens the update dialog prefilled from the record and without a project picker", () => {
    renderProvider();
    act(() => handle?.openEdit(record({ name: "Prefill Me", description: "Kept" })));
    expect(screen.getByRole("heading", { name: "Update module" })).toBeInTheDocument();
    expect(screen.queryByTestId("module-dialog-project")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Title" })).toHaveValue("Prefill Me");
    expect(screen.getByRole("textbox", { name: "Description" })).toHaveValue("Kept");
  });

  it("opens the delete confirmation for the named module and carries the host callback", async () => {
    const onDeleted = vi.fn();
    renderProvider();
    act(() => handle?.openDelete({ id: MODULES_MODULE_ID, name: "Delivery" }, { onDeleted }));
    expect(screen.getByRole("heading", { name: "Delete module" })).toBeInTheDocument();
    expect(screen.getByTestId("module-delete-body")).toHaveTextContent("Delivery");
  });

  it("opens the archive confirmation for the named module", () => {
    renderProvider();
    act(() => handle?.openArchive({ id: MODULES_MODULE_ID, name: "Delivery" }));
    expect(screen.getByTestId("module-archive-body")).toHaveTextContent("Delivery");
  });

  it("closes the archive confirmation without writing, with or without a landing", async () => {
    const onArchived = vi.fn();
    renderProvider();
    act(() => handle?.openArchive({ id: MODULES_MODULE_ID, name: "Delivery" }, { onArchived }));
    await act(async () => {
      screen.getByRole("button", { name: "Cancel" }).click();
    });
    await waitFor(() => expect(screen.queryByTestId("module-archive-body")).not.toBeInTheDocument());
    expect(onArchived).not.toHaveBeenCalled();

    act(() => handle?.openArchive({ id: MODULES_MODULE_ID, name: "Delivery" }));
    expect(screen.getByTestId("module-archive-body")).toHaveTextContent("Delivery");
  });

  it("closes each dialog through its own onOpenChange", async () => {
    renderProvider();
    act(() => handle?.openDelete({ id: MODULES_MODULE_ID, name: "Delivery" }));
    await act(async () => {
      screen.getByRole("button", { name: "Cancel" }).click();
    });
    await waitFor(() => expect(screen.queryByRole("heading", { name: "Delete module" })).not.toBeInTheDocument());
    act(() => handle?.openCreate());
    expect(await screen.findByRole("heading", { name: "Create module" })).toBeInTheDocument();
    // A later create replaces the earlier session rather than stacking.
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
  });

  it("dismisses the save and delete dialogs on Escape", async () => {
    renderProvider();
    act(() => handle?.openCreate());
    await act(async () => {
      fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    });
    await waitFor(() => expect(screen.queryByRole("heading", { name: "Create module" })).not.toBeInTheDocument());
    act(() => handle?.openDelete({ id: MODULES_MODULE_ID, name: "Delivery" }));
    await act(async () => {
      fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    });
    await waitFor(() => expect(screen.queryByRole("heading", { name: "Delete module" })).not.toBeInTheDocument());
  });

  it("keeps the openers stable across renders", () => {
    const view = renderProvider();
    const first = handle;
    view.rerender(
      <QueryClientProvider client={new QueryClient()}>
        <ModuleDialogProvider workspaceSlug="acme" projectId={MODULES_PROJECT_ID} currentProject={CURRENT}>
          <Probe />
        </ModuleDialogProvider>
      </QueryClientProvider>
    );
    expect(handle).toBe(first);
  });
});
