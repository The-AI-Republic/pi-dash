// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archive confirmation (parity/specs/modules.md §7.4, MOD-026): the copy
// names the module and promises a restore; confirming archives, refreshes
// the module reads, notices the move and lands on the modules list (or
// leaves it to the host); cancel never writes; a failure reports an error
// and stays put, because the module is still live (NEWFRONT-233).

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

const router = vi.hoisted(() => ({ navigate: vi.fn() }));
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useNavigate: () => router.navigate,
}));

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { moduleKeys } from "../../api/keys.js";
import {
  MODULE_ARCHIVE_SUCCESS_BODY,
  MODULE_ARCHIVE_SUCCESS_TITLE,
  moduleArchiveDialogBody,
  moduleArchiveDialogTitle,
} from "../../lib/moduleArchive.js";
import { jsonResponse, MODULES_MODULE_ID, MODULES_PROJECT_ID } from "../../testing.js";
import { ModuleArchiveDialog } from "./ModuleArchiveDialog.js";

const PATH = `/api/workspaces/acme/projects/${MODULES_PROJECT_ID}/modules/${MODULES_MODULE_ID}/archive/`;

interface Harness {
  status: number;
  body: unknown;
  calls: string[];
  delay: Promise<void> | null;
}

let harness: Harness;

beforeEach(() => {
  harness = { status: 200, body: null, calls: [], delay: null };
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (init.method === "POST" && url.endsWith(PATH)) {
      harness.calls.push(url);
      if (harness.delay !== null) await harness.delay;
      if (harness.status === 200) return jsonResponse(url, 200, { archived_at: "2026-03-01T00:00:00Z" });
      return jsonResponse(url, harness.status, harness.body);
    }
    throw new Error(`unexpected request: ${init.method ?? "GET"} ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  notify.notifyError.mockReset();
  notify.notifySuccess.mockReset();
  router.navigate.mockReset();
  router.navigate.mockResolvedValue(undefined);
  globalThis.window.history.replaceState(null, "", "/acme/projects/p1/issues");
});

function renderDialog(overrides: Partial<React.ComponentProps<typeof ModuleArchiveDialog>> = {}) {
  const onClose = vi.fn();
  const onArchived = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const ui = (
    <QueryClientProvider client={queryClient}>
      <ModuleArchiveDialog
        module={{ id: MODULES_MODULE_ID, name: "Atlas" }}
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        onClose={onClose}
        {...overrides}
      />
    </QueryClientProvider>
  );
  const view = render(ui);
  return { onClose, onArchived, queryClient, ...view };
}

describe("ModuleArchiveDialog", () => {
  it("stays closed while no module is targeted", () => {
    renderDialog({ module: null });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("names the module and says it can be restored", () => {
    renderDialog();
    const heading = screen.getByRole("heading", { name: moduleArchiveDialogTitle("Atlas") });
    expect(heading).toBeInTheDocument();
    const copy = screen.getByTestId("module-archive-body");
    expect(copy).toHaveTextContent("Atlas");
    expect(copy).toHaveTextContent("restored");
    expect(copy).toHaveTextContent(moduleArchiveDialogBody("Atlas"));
  });

  it("cancels without writing", () => {
    const { onClose, onArchived } = renderDialog({ onArchived: vi.fn() });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalled();
    expect(onArchived).not.toHaveBeenCalled();
    expect(harness.calls).toHaveLength(0);
    expect(router.navigate).not.toHaveBeenCalled();
  });

  it("closes on a backdrop press without writing", async () => {
    const { onClose } = renderDialog();
    const backdrop = document.querySelector("[data-base-ui-backdrop]") ?? document.body;
    fireEvent.click(backdrop);
    expect(onClose).toHaveBeenCalled();
    expect(harness.calls).toHaveLength(0);
  });

  it("archives, refreshes the module reads, notices the move and lands on the modules list", async () => {
    const { onClose, queryClient } = renderDialog();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    fireEvent.click(screen.getByRole("button", { name: "Archive" }));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(harness.calls).toEqual([`https://api.test${PATH}`]);
    expect(notify.notifySuccess).toHaveBeenCalledWith(MODULE_ARCHIVE_SUCCESS_TITLE, MODULE_ARCHIVE_SUCCESS_BODY);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: moduleKeys.list("acme", MODULES_PROJECT_ID) });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: moduleKeys.archivedList("acme", MODULES_PROJECT_ID) });
    // The typed route target keeps the list's trailing slash; a plain
    // string target loses it in the router.
    expect(router.navigate).toHaveBeenCalledWith({
      to: "/$ws/projects/$projectId/modules/",
      params: { ws: "acme", projectId: MODULES_PROJECT_ID },
      search: {},
      replace: true,
    });
    expect(notify.notifyError).not.toHaveBeenCalled();
  });

  it("rewrites a slashless modules address after landing, which only the slash distinguishes", async () => {
    // The router sees `/modules` and `/modules/` as one location, so the
    // landing navigation is a no-op and the address keeps its entered
    // form; the dialog rewrites it to the canonical list address.
    router.navigate.mockImplementation(async ({ params }: { params: { ws: string; projectId: string } }) => {
      globalThis.window.history.replaceState(null, "", `/${params.ws}/projects/${params.projectId}/modules`);
    });
    renderDialog();
    fireEvent.click(screen.getByRole("button", { name: "Archive" }));
    await waitFor(() =>
      expect(globalThis.window.location.pathname).toBe(`/acme/projects/${MODULES_PROJECT_ID}/modules/`)
    );
  });

  it("leaves an already-canonical address alone", async () => {
    router.navigate.mockImplementation(async ({ params }: { params: { ws: string; projectId: string } }) => {
      globalThis.window.history.replaceState(null, "", `/${params.ws}/projects/${params.projectId}/modules/`);
    });
    renderDialog();
    fireEvent.click(screen.getByRole("button", { name: "Archive" }));
    await waitFor(() => expect(router.navigate).toHaveBeenCalled());
    expect(globalThis.window.location.pathname).toBe(`/acme/projects/${MODULES_PROJECT_ID}/modules/`);
  });

  it("hands the landed archive to a host that supplies its own landing", async () => {
    const onArchived = vi.fn();
    renderDialog({ onArchived });
    fireEvent.click(screen.getByRole("button", { name: "Archive" }));
    await waitFor(() => expect(onArchived).toHaveBeenCalledWith({ id: MODULES_MODULE_ID, name: "Atlas" }));
    expect(router.navigate).not.toHaveBeenCalled();
  });

  it("reports a failed archive and never claims success or navigates", async () => {
    harness.status = 400;
    harness.body = { error: "Only completed or cancelled modules can be archived" };
    const { onClose } = renderDialog();
    fireEvent.click(screen.getByRole("button", { name: "Archive" }));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(notify.notifyError).toHaveBeenCalledWith(
      "Error!",
      expect.stringContaining("Only completed or cancelled modules can be archived")
    );
    expect(notify.notifySuccess).not.toHaveBeenCalled();
    expect(router.navigate).not.toHaveBeenCalled();
  });

  it("keeps the confirmation up while the archive flies", async () => {
    let release: () => void = () => undefined;
    harness.delay = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { onClose } = renderDialog();
    fireEvent.click(screen.getByRole("button", { name: "Archive" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Archive" })).toBeDisabled());
    expect(onClose).not.toHaveBeenCalled();
    release();
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });
});
