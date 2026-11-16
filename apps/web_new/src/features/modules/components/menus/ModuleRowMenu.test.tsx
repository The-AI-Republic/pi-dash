// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One module's quick menu (MOD-017/018): the entries per role and
// archive state, the archive gate with its explanation, and every
// behavior — the shared edit/delete dialogs, the link actions, the
// archive handoff slice G wires, and restore with its own notices.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ModuleRow } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const notify = vi.hoisted(() => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));
vi.mock("../../../../shared/shell/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../shared/shell/index.js")>()),
  notifyError: notify.notifyError,
  notifySuccess: notify.notifySuccess,
}));

const share = vi.hoisted(() => ({ copyModuleLink: vi.fn(), openModuleInNewTab: vi.fn() }));
vi.mock("../../lib/moduleShare.js", () => ({
  copyModuleLink: share.copyModuleLink,
  openModuleInNewTab: share.openModuleInNewTab,
}));

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { jsonResponse, makeModuleRow, MODULES_MODULE_ID, MODULES_PROJECT_ID } from "../../testing.js";
import { ModuleDialogProvider } from "../dialogs/ModuleDialogProvider.js";
import { MODULE_RESTORE_SUCCESS_BODY, MODULE_RESTORE_SUCCESS_TITLE, ModuleRowMenu } from "./ModuleRowMenu.js";

const WS = "acme";
const ARCHIVE_PATH = `/api/workspaces/${WS}/projects/${MODULES_PROJECT_ID}/modules/${MODULES_MODULE_ID}/archive/`;
const DETAIL_PATH = `/api/workspaces/${WS}/projects/${MODULES_PROJECT_ID}/modules/${MODULES_MODULE_ID}/`;

let harness: { restoreStatus: number; restoreDetail: string };

function renderMenu(props: Partial<React.ComponentProps<typeof ModuleRowMenu>> = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const invalidate = vi.spyOn(client, "invalidateQueries");
  render(
    <QueryClientProvider client={client}>
      <ModuleDialogProvider
        workspaceSlug={WS}
        projectId={MODULES_PROJECT_ID}
        currentProject={{ id: MODULES_PROJECT_ID, name: "Acme", identifier: "AC" }}
      >
        <ModuleRowMenu workspaceSlug={WS} projectId={MODULES_PROJECT_ID} module={makeModuleRow()} canEdit {...props} />
      </ModuleDialogProvider>
    </QueryClientProvider>
  );
  return { invalidate };
}

function entryTitles(): string[] {
  return screen
    .getAllByRole("menuitem")
    .map((entry) => entry.querySelector("[data-menu-title]")?.textContent?.trim() ?? entry.textContent?.trim() ?? "");
}

async function openMenu(name = "Delivery"): Promise<void> {
  fireEvent.click(await screen.findByRole("button", { name: `Module actions for ${name}` }));
  // Every menu variant offers "Open in new tab" exactly once.
  await screen.findByRole("menuitem", { name: "Open in new tab" });
}

beforeEach(() => {
  harness = { restoreStatus: 204, restoreDetail: "" };
  createAppClient("https://api.test", (async (url: string, init: { method?: string }) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url === `https://api.test${ARCHIVE_PATH}` && (init.method ?? "GET") === "DELETE") {
      if (harness.restoreStatus === 204) return jsonResponse(url, 204, null);
      return jsonResponse(url, harness.restoreStatus, { detail: harness.restoreDetail });
    }
    if (url === `https://api.test${DETAIL_PATH}` && init.method === "DELETE") {
      return jsonResponse(url, 204, null);
    }
    if (url === "https://api.test/api/workspaces/acme/projects/") return jsonResponse(url, 200, []);
    if (url.endsWith("/api/workspaces/acme/members/")) return jsonResponse(url, 200, []);
    if (url.endsWith("/api/users/me/")) return jsonResponse(url, 500, {});
    return jsonResponse(url, 500, {});
  }) as never);
});

afterEach(() => {
  resetAppClient();
  notify.notifyError.mockReset();
  notify.notifySuccess.mockReset();
  share.copyModuleLink.mockReset();
  share.openModuleInNewTab.mockReset();
});

describe("ModuleRowMenu", () => {
  it("marks the trigger for the driver and reveals it on hover by default", () => {
    renderMenu();
    const trigger = screen.getByRole("button", { name: "Module actions for Delivery" });
    expect(trigger).toHaveAttribute("data-testid", "module-row-menu-trigger");
    expect(trigger.closest('[data-main-menu="true"]')).not.toBe(null);
    expect(trigger.className).toContain("group-hover:opacity-100");
  });

  it("keeps the trigger visible on cards and narrow rows", () => {
    renderMenu({ alwaysVisible: true });
    expect(screen.getByRole("button", { name: "Module actions for Delivery" }).className).not.toContain(
      "group-hover:opacity-100"
    );
  });

  it("assembles the editor entries with the archive gate explained", async () => {
    renderMenu({ module: makeModuleRow({ status: "planned" }) });
    await openMenu();
    expect(entryTitles()).toEqual(["Edit", "Open in new tab", "Copy link", "Archive", "Delete"]);
    const archive = screen.getByRole("menuitem", { name: /Archive/ });
    expect(archive).toHaveAttribute("data-disabled");
    expect(archive.textContent ?? "").toContain("completed or cancelled");
  });

  it("enables archive once the status finishes", async () => {
    renderMenu({ module: makeModuleRow({ status: "cancelled" }) });
    await openMenu();
    expect(screen.getByRole("menuitem", { name: "Archive" })).not.toHaveAttribute("data-disabled");
  });

  it("offers the link actions and restore on archived modules", async () => {
    renderMenu({ archived: true, module: makeModuleRow({ status: "completed" }) });
    await openMenu();
    expect(entryTitles()).toEqual(["Open in new tab", "Copy link", "Restore"]);
    expect(screen.queryByRole("menuitem", { name: "Edit" })).not.toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: /Delete/ })).not.toBeInTheDocument();
  });

  it("offers only the link actions to guests", async () => {
    renderMenu({ canEdit: false });
    await openMenu();
    expect(entryTitles()).toEqual(["Open in new tab", "Copy link"]);
  });

  it("opens the shared edit dialog prefilled from the row", async () => {
    renderMenu();
    await openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Edit" }));
    expect(await screen.findByRole("heading", { name: "Update module" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Title" })).toHaveValue("Delivery");
  });

  it("opens the shared delete dialog and stays on the list once it lands", async () => {
    const { invalidate } = renderMenu();
    await openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    expect(await screen.findByRole("heading", { name: "Delete module" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(notify.notifySuccess).toHaveBeenCalledWith("Success!", expect.stringContaining("Delivery"))
    );
    expect(screen.queryByRole("heading", { name: "Delete module" })).not.toBeInTheDocument();
    expect(invalidate).toHaveBeenCalled();
  });

  it("reports a landed delete to the host", async () => {
    const onDeleted = vi.fn();
    renderMenu({ onDeleted });
    await openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    expect(await screen.findByRole("heading", { name: "Delete module" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(onDeleted).toHaveBeenCalledWith(expect.objectContaining({ id: MODULES_MODULE_ID, name: "Delivery" }))
    );
  });

  it("copies the link and opens new tabs through the shared helpers", async () => {
    renderMenu();
    await openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    expect(share.copyModuleLink).toHaveBeenCalledWith(WS, MODULES_PROJECT_ID, MODULES_MODULE_ID);
    await openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Open in new tab" }));
    expect(share.openModuleInNewTab).toHaveBeenCalledWith(WS, MODULES_PROJECT_ID, MODULES_MODULE_ID);
  });

  it("hands archive to the opener slice G wires", async () => {
    const onArchive = vi.fn();
    renderMenu({ module: makeModuleRow({ status: "completed" }), onArchive });
    await openMenu();
    const target: ModuleRow = makeModuleRow({ status: "completed" });
    fireEvent.click(screen.getByRole("menuitem", { name: "Archive" }));
    expect(onArchive).toHaveBeenCalledWith(target);
  });

  it("leaves archive with nowhere to open until slice G wires it", async () => {
    renderMenu({ module: makeModuleRow({ status: "completed" }) });
    await openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Archive" }));
    expect(notify.notifySuccess).not.toHaveBeenCalled();
    expect(notify.notifyError).not.toHaveBeenCalled();
  });

  it("restores with a notice and refreshes the lists", async () => {
    const { invalidate } = renderMenu({ archived: true });
    await openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Restore" }));
    await waitFor(() =>
      expect(notify.notifySuccess).toHaveBeenCalledWith(MODULE_RESTORE_SUCCESS_TITLE, MODULE_RESTORE_SUCCESS_BODY)
    );
    expect(notify.notifyError).not.toHaveBeenCalled();
    expect(invalidate).toHaveBeenCalled();
  });

  it("reports a refused restore and keeps the module archived", async () => {
    harness.restoreStatus = 500;
    harness.restoreDetail = "Locked.";
    renderMenu({ archived: true });
    await openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Restore" }));
    await waitFor(() => expect(notify.notifyError).toHaveBeenCalledWith("Error!", "Locked."));
    expect(notify.notifySuccess).not.toHaveBeenCalled();
  });

  it("renders the read-only entries outside the dialog context without writing", async () => {
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <ModuleRowMenu workspaceSlug={WS} projectId={MODULES_PROJECT_ID} module={makeModuleRow()} canEdit />
      </QueryClientProvider>
    );
    await openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Edit" }));
    expect(screen.queryByRole("heading", { name: "Update module" })).not.toBeInTheDocument();
    await openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    expect(screen.queryByRole("heading", { name: "Delete module" })).not.toBeInTheDocument();
    await openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Copy link" }));
    expect(share.copyModuleLink).toHaveBeenCalledTimes(1);
  });
});
