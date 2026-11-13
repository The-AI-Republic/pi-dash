// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cross-project move dialog (parity/specs/pages.md §3.8, PAGE-017,
// PDASHOSS01-347). The picker offers only the other live projects the
// viewer can write to, the confirm control stays disabled until a target is
// chosen, a successful confirm hands the id to the host, and a pending
// write pins both controls.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { PAGE_MOVE_NO_TARGETS, PageMoveDialog, type PageMoveDialogProps } from "./PageMoveDialog.js";

const CURRENT = "223e4567-e89b-12d3-a456-426614174001";
const OTHER = "33333333-3333-4333-8333-333333333333";
const WORKSPACE = "44444444-4444-4444-8444-444444444444";
const ARCHIVED = "55555555-5555-4555-8555-555555555555";
const GUESTED = "66666666-6666-4666-8666-666666666666";
const OUTSIDE = "77777777-7777-4777-8777-777777777777";
const MEMBERED = "88888888-8888-4888-8888-888888888888";

function projectRow(
  id: string,
  name: string,
  overrides: { member_role?: number | null; archived_at?: string | null } = {}
): Record<string, unknown> {
  return {
    id,
    name,
    identifier: name.slice(0, 4).toUpperCase(),
    sort_order: null,
    logo_props: {},
    member_role: overrides.member_role === undefined ? 20 : overrides.member_role,
    intake_count: 0,
    archived_at: overrides.archived_at ?? null,
    workspace: WORKSPACE,
    cycle_view: false,
    issue_views_view: false,
    module_view: false,
    page_view: false,
    inbox_view: false,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 0,
    created_at: "2026-01-01T00:00:00.000Z",
    updated_at: "2026-01-01T00:00:00.000Z",
    created_by: null,
    updated_by: null,
  };
}

const DEFAULT_PROJECTS = [projectRow(CURRENT, "Current project"), projectRow(OTHER, "Target project")];

function harness(projects: Record<string, unknown>[] = DEFAULT_PROJECTS): QueryClient {
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"csrf"}' };
    }
    return {
      status: 200,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(projects),
    };
  }) as never);
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

function renderDialog(
  overrides: Partial<PageMoveDialogProps> = {},
  projects?: Record<string, unknown>[]
): {
  onOpenChange: ReturnType<typeof vi.fn>;
  onConfirm: ReturnType<typeof vi.fn>;
  queryClient: QueryClient;
} {
  const queryClient = harness(projects);
  const onOpenChange = vi.fn();
  const onConfirm = vi.fn();
  render(
    <QueryClientProvider client={queryClient}>
      <PageMoveDialog
        open
        onOpenChange={onOpenChange}
        workspaceSlug="acme"
        currentProjectId={CURRENT}
        pageName="Release checklist"
        pending={false}
        onConfirm={onConfirm}
        {...overrides}
      />
    </QueryClientProvider>
  );
  return { onOpenChange, onConfirm, queryClient };
}

afterEach(() => {
  resetAppClient();
});

describe("PageMoveDialog", () => {
  it("heads the dialog Move page, names the page and keeps confirm disabled until a target is chosen", async () => {
    renderDialog();
    expect(screen.getByRole("heading", { name: "Move page" })).toBeInTheDocument();
    expect(screen.getByRole("dialog")).toHaveTextContent("Release checklist");
    const confirm = screen.getByRole("button", { name: "Move page" });
    expect(confirm).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "Select a project" }));
    const listbox = await screen.findByRole("listbox");
    const names = within(listbox)
      .getAllByRole("option")
      .map((option) => option.textContent ?? "");
    expect(names.join(" ")).toContain("Target project");
    expect(names.join(" ")).not.toContain("Current project");
    await waitFor(() => expect(screen.getByRole("button", { name: "Move page" })).toBeDisabled());
  });

  it("hands the chosen project to the host and closes", async () => {
    const user = userEvent.setup();
    const { onConfirm, onOpenChange } = renderDialog();
    fireEvent.click(screen.getByRole("button", { name: "Select a project" }));
    fireEvent.click(await screen.findByRole("option", { name: /Target project/ }));
    const confirm = screen.getByRole("button", { name: "Move page" });
    expect(confirm).toBeEnabled();
    await user.click(confirm);
    expect(onConfirm).toHaveBeenCalledWith(OTHER);
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("cancels to closed and does not move", async () => {
    const user = userEvent.setup();
    const { onConfirm, onOpenChange } = renderDialog();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("pins both controls while the move flies", async () => {
    const user = userEvent.setup();
    const { onConfirm, onOpenChange } = renderDialog({ pending: true });
    const confirm = screen.getByRole("button", { name: "Moving…" });
    expect(confirm).toBeDisabled();
    await user.click(confirm);
    expect(onConfirm).not.toHaveBeenCalled();
    const cancel = screen.getByRole("button", { name: "Cancel" });
    expect(cancel).toBeDisabled();
    await user.click(cancel);
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("dismisses on Escape when no write is running", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = renderDialog();
    await user.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("refuses to dismiss while the request is in flight", async () => {
    const user = userEvent.setup();
    const { onOpenChange } = renderDialog({ pending: true });
    await user.keyboard("{Escape}");
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("names an untitled page with the fallback label", () => {
    renderDialog({ pageName: "" });
    expect(screen.getByRole("dialog")).toHaveTextContent("Untitled");
  });

  it("offers only the other live projects the viewer can write to", async () => {
    renderDialog({}, [
      projectRow(CURRENT, "Current project"),
      projectRow(OTHER, "Target project"),
      projectRow(MEMBERED, "Member project", { member_role: 15 }),
      projectRow(ARCHIVED, "Archived project", { archived_at: "2026-02-01T00:00:00.000Z" }),
      projectRow(GUESTED, "Guest project", { member_role: 5 }),
      projectRow(OUTSIDE, "Outside project", { member_role: null }),
    ]);
    fireEvent.click(screen.getByRole("button", { name: "Select a project" }));
    const listbox = await screen.findByRole("listbox");
    await waitFor(() => expect(within(listbox).getAllByRole("option")).toHaveLength(2));
    const names = within(listbox)
      .getAllByRole("option")
      .map((option) => option.textContent ?? "")
      .join(" ");
    expect(names).toContain("Target project");
    expect(names).toContain("Member project");
    for (const hidden of ["Current project", "Archived project", "Guest project", "Outside project"]) {
      expect(names).not.toContain(hidden);
    }
  });

  it("says so when there is no project to move the page into", async () => {
    renderDialog({}, [
      projectRow(CURRENT, "Current project"),
      projectRow(GUESTED, "Guest project", { member_role: 5 }),
    ]);
    expect(await screen.findByTestId("page-move-no-targets")).toHaveTextContent(PAGE_MOVE_NO_TARGETS);
    expect(screen.getByRole("button", { name: "Select a project" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Move page" })).toBeDisabled();
  });
});
