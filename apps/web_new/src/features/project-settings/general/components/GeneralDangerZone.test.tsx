// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Danger zone (parity/specs/project-settings-general.md §Danger zone;
// PSET-010–012): admin-only controls, archive through the shared confirm
// dialog, delete through the double-typed confirmation, and a return to
// the projects list after either write lands.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const routerMocks = vi.hoisted(() => ({ navigate: vi.fn() }));

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useNavigate: () => routerMocks.navigate,
}));

vi.mock("../../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { notifySuccess } from "../../../../shared/shell/index.js";
import { DELETE_CONFIRM_PHRASE } from "../../../projects/index.js";
import { GeneralDangerZone } from "./GeneralDangerZone.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";

function stub(status = 204) {
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    return {
      status,
      url,
      headers: { get: () => null },
      text: async () => (url.includes("/archive/") ? JSON.stringify({ archived_at: "2026-10-21T00:00:00Z" }) : "{}"),
    };
  }) as never);
}

function setup(status = 204) {
  stub(status);
  const onNavigate = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <GeneralDangerZone
        workspaceSlug="acme"
        projectId={PROJECT_ID}
        projectName="Website relaunch"
        onNavigate={onNavigate}
      />
    </QueryClientProvider>
  );
  return { onNavigate };
}

function setupDefaultNavigation(status = 204) {
  stub(status);
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <GeneralDangerZone workspaceSlug="acme" projectId={PROJECT_ID} projectName="Website relaunch" />
    </QueryClientProvider>
  );
}

beforeEach(() => vi.clearAllMocks());
afterEach(() => resetAppClient());

describe("GeneralDangerZone", () => {
  it("renders the archive and delete rows for an admin", () => {
    setup();
    expect(screen.getByRole("region", { name: "Danger zone" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Archive" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete" })).toBeInTheDocument();
  });

  it("archives from the confirm dialog and returns to the project list", async () => {
    const { onNavigate } = setup();
    fireEvent.click(screen.getByRole("button", { name: "Archive" }));
    expect(screen.getByRole("heading", { name: "Archive Website relaunch" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Archive" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalled());
    expect(onNavigate).toHaveBeenCalledWith("/acme/projects");
  });

  it("navigates client-side by default so the success notice survives", async () => {
    // PSET-010: a full reload would wipe the archive notice before the
    // operator can read it, so the default return is a router move.
    setupDefaultNavigation();
    fireEvent.click(screen.getByRole("button", { name: "Archive" }));
    fireEvent.click(screen.getByRole("button", { name: "Archive" }));
    await waitFor(() =>
      expect(routerMocks.navigate).toHaveBeenCalledWith({ to: "/$ws/projects", params: { ws: "acme" } })
    );
  });

  it("deletes only after both exact confirmations and then returns to the list", async () => {
    const { onNavigate } = setup();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    const submit = screen.getByRole("button", { name: "Delete project" });
    expect(submit).toBeDisabled();
    fireEvent.change(screen.getByPlaceholderText("Project name"), { target: { value: "Website relaunch" } });
    expect(submit).toBeDisabled();
    fireEvent.change(screen.getByPlaceholderText(`Enter '${DELETE_CONFIRM_PHRASE}'`), {
      target: { value: DELETE_CONFIRM_PHRASE },
    });
    expect(submit).not.toBeDisabled();
    fireEvent.click(submit);
    await waitFor(() => expect(onNavigate).toHaveBeenCalledWith("/acme/projects"));
  });
});
