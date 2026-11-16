// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Views dialog absence proofs (VIEW-043, VIEW-044, from
// parity/specs/views.md §13): every save dialog tabs in field order
// (title, description, the remaining controls, Cancel, submit) with
// no positive tabindex anywhere; a live Tab from the title lands on
// the description (NEWFRONT-241's old skip, pinned as the keypress
// itself); the delete confirmations order Cancel before their danger
// action; and the acting user's own project create adopts the created
// row into the list cache without a refetch. Escape itself is pinned
// per dialog in the owning suites; the node scan beside this holds
// the no-other-handlers half of VIEW-044 across every views source.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
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
import { auditViewAbsence } from "../absence/viewAbsence.js";
import { viewKeys } from "../api/keys.js";
import { CreateViewDialog, DeleteWorkspaceViewDialog, UpdateViewDialog, WorkspaceViewDialog } from "../index.js";
import { makeView, VIEWS_OWNER_ID } from "../testing.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const VIEW_ID = "323e4567-e89b-12d3-a456-426614174002";
const OTHER_ID = "623e4567-e89b-12d3-a456-426614174005";

const CREATED = makeView({ id: VIEW_ID, project: PROJECT, name: "Triage" });
const STORED = makeView({ id: OTHER_ID, project: PROJECT, name: "Alpha", description: "kept row" });

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

function renderSaveDialog(ui: React.ReactElement): QueryClient {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(<QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>);
  return queryClient;
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

/**
 * The announced names of the tabbable controls in DOM order. Tab
 * follows DOM order unless a positive tabindex says otherwise, which
 * the companion assertion forbids, so this order is the tab order.
 */
function tabbableNames(root: ParentNode): string[] {
  const names: string[] = [];
  root.querySelectorAll("input, textarea, select, button, a[href]").forEach((element) => {
    if (element.hasAttribute("disabled")) return;
    if (element.getAttribute("tabindex") === "-1") return;
    names.push(
      element.getAttribute("aria-label") ||
        (element as HTMLInputElement).placeholder ||
        (element.textContent ?? "").trim()
    );
  });
  return names;
}

function expectNoPositiveTabindex(root: ParentNode): void {
  for (const element of root.querySelectorAll("[tabindex]")) {
    expect(Number(element.getAttribute("tabindex"))).toBeLessThanOrEqual(0);
  }
}

/**
 * The announcement states the full order: title, description, the
 * remaining form controls, then Cancel and the submit action last.
 */
function expectFieldOrder(root: ParentNode, submit: string): void {
  const names = tabbableNames(root);
  expect(names[0], `title leads: ${names.join(" | ")}`).toBe("Title");
  expect(names[1], `description follows: ${names.join(" | ")}`).toBe("Description");
  expect(names.indexOf("Title", 1)).toBe(-1);
  expect(names.indexOf("Description", 2)).toBe(-1);
  expect(names.slice(-2), `footer trails: ${names.join(" | ")}`).toEqual(["Cancel", submit]);
}

/**
 * NEWFRONT-241's exact repro as a live traversal: from the focused
 * title, one Tab must land on the description. The old project dialog
 * skipped it (title, Cancel, submit, wrap); the DOM-order suite above
 * infers the order, this pins the keypress itself, inside the dialog.
 */
async function expectTabReachesDescription(ui: React.ReactElement, status: number, body: unknown): Promise<void> {
  const user = userEvent.setup();
  seedSession();
  transport(() => ({ status, body }));
  renderSaveDialog(ui);
  await screen.findByTestId("rich-filter-add");
  const dialog = screen.getByRole("dialog");
  const title = screen.getByPlaceholderText("Title");
  await user.click(title);
  expect(title).toHaveFocus();
  await user.tab();
  // Base UI's focus guard hands focus over asynchronously.
  await waitFor(() => expect(screen.getByPlaceholderText("Description")).toHaveFocus());
  expect(dialog.contains(document.activeElement)).toBe(true);
}

describe("save dialog tab order (VIEW-044)", () => {
  it("tabs the project create dialog in field order", async () => {
    seedSession();
    transport(() => ({ status: 201, body: CREATED }));
    renderSaveDialog(
      <CreateViewDialog
        open={true}
        onOpenChange={() => undefined}
        workspaceSlug="acme"
        projectId={PROJECT}
        hideCycle={false}
        hideModule={false}
      />
    );
    expect(await screen.findByTestId("rich-filter-add")).toBeInTheDocument();
    expectFieldOrder(document, "Create View");
    expectNoPositiveTabindex(document);
  });

  it("tabs the project update dialog in field order", async () => {
    seedSession();
    transport(() => ({ status: 200, body: STORED }));
    renderSaveDialog(
      <UpdateViewDialog
        open={true}
        onOpenChange={() => undefined}
        workspaceSlug="acme"
        projectId={PROJECT}
        hideCycle={false}
        hideModule={false}
        view={STORED}
      />
    );
    expect(await screen.findByTestId("rich-filter-add")).toBeInTheDocument();
    expectFieldOrder(document, "Update View");
    expectNoPositiveTabindex(document);
  });

  it("tabs the workspace create dialog in field order", async () => {
    seedSession();
    transport(() => ({ status: 201, body: CREATED }));
    renderSaveDialog(
      <WorkspaceViewDialog open={true} onOpenChange={() => undefined} workspaceSlug="acme" mode="create" />
    );
    expect(await screen.findByTestId("rich-filter-add")).toBeInTheDocument();
    expectFieldOrder(document, "Create View");
    expectNoPositiveTabindex(document);
  });

  it("tabs the workspace edit dialog in field order", async () => {
    seedSession();
    transport(() => ({ status: 200, body: STORED }));
    renderSaveDialog(
      <WorkspaceViewDialog open={true} onOpenChange={() => undefined} workspaceSlug="acme" mode="edit" view={STORED} />
    );
    expect(await screen.findByTestId("rich-filter-add")).toBeInTheDocument();
    expectFieldOrder(document, "Update View");
    expectNoPositiveTabindex(document);
  });
});

describe("live Tab reaches Description (NEWFRONT-241, VIEW-044)", () => {
  it("tabs the project create dialog from Title into Description", async () => {
    await expectTabReachesDescription(
      <CreateViewDialog
        open={true}
        onOpenChange={() => undefined}
        workspaceSlug="acme"
        projectId={PROJECT}
        hideCycle={false}
        hideModule={false}
      />,
      201,
      CREATED
    );
  });

  it("tabs the project update dialog from Title into Description", async () => {
    await expectTabReachesDescription(
      <UpdateViewDialog
        open={true}
        onOpenChange={() => undefined}
        workspaceSlug="acme"
        projectId={PROJECT}
        hideCycle={false}
        hideModule={false}
        view={STORED}
      />,
      200,
      STORED
    );
  });

  it("tabs the workspace create dialog from Title into Description", async () => {
    await expectTabReachesDescription(
      <WorkspaceViewDialog open={true} onOpenChange={() => undefined} workspaceSlug="acme" mode="create" />,
      201,
      CREATED
    );
  });

  it("tabs the workspace edit dialog from Title into Description", async () => {
    await expectTabReachesDescription(
      <WorkspaceViewDialog open={true} onOpenChange={() => undefined} workspaceSlug="acme" mode="edit" view={STORED} />,
      200,
      STORED
    );
  });
});

describe("delete confirmation order (VIEW-044)", () => {
  it("orders Cancel before the danger action", async () => {
    seedSession();
    transport(() => ({ status: 200, body: {} }));
    renderSaveDialog(
      <DeleteWorkspaceViewDialog open={true} onOpenChange={() => undefined} workspaceSlug="acme" view={STORED} />
    );
    expect(await screen.findByRole("button", { name: "Delete" })).toBeInTheDocument();
    const names = tabbableNames(document);
    expect(names.indexOf("Cancel")).toBeGreaterThanOrEqual(0);
    expect(names.indexOf("Delete")).toBeGreaterThan(names.indexOf("Cancel"));
    expectNoPositiveTabindex(document);
  });
});

describe("own project create updates the list (VIEW-043)", () => {
  it("adopts the created row into a fetched list without a refetch", async () => {
    const user = userEvent.setup();
    seedSession();
    const calls = transport(() => ({ status: 201, body: CREATED }));
    const queryClient = renderSaveDialog(
      <CreateViewDialog
        open={true}
        onOpenChange={() => undefined}
        workspaceSlug="acme"
        projectId={PROJECT}
        hideCycle={false}
        hideModule={false}
      />
    );
    queryClient.setQueryData(viewKeys.projectList("acme", PROJECT), [STORED]);
    expect(await screen.findByTestId("rich-filter-add")).toBeInTheDocument();
    await user.type(screen.getByPlaceholderText("Title"), "Triage");
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith("View created successfully"));
    expect(queryClient.getQueryData(viewKeys.projectList("acme", PROJECT))).toEqual([CREATED, STORED]);
    expect(calls.map((call) => call.method)).toEqual(["POST"]);
  });

  it("leaves a never-fetched list unfetched so it reads fresh on mount", async () => {
    const user = userEvent.setup();
    seedSession();
    transport(() => ({ status: 201, body: CREATED }));
    const queryClient = renderSaveDialog(
      <CreateViewDialog
        open={true}
        onOpenChange={() => undefined}
        workspaceSlug="acme"
        projectId={PROJECT}
        hideCycle={false}
        hideModule={false}
      />
    );
    expect(await screen.findByTestId("rich-filter-add")).toBeInTheDocument();
    await user.type(screen.getByPlaceholderText("Title"), "Triage");
    await user.click(screen.getByRole("button", { name: "Create View" }));
    await waitFor(() => expect(notify.notifySuccess).toHaveBeenCalledWith("View created successfully"));
    expect(queryClient.getQueryData(viewKeys.projectList("acme", PROJECT))).toBeUndefined();
  });
});

describe("save dialog absence (VIEW-047)", () => {
  it("audits the open save dialogs to zeros", async () => {
    seedSession();
    transport(() => ({ status: 201, body: CREATED }));
    const { unmount } = render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <CreateViewDialog
          open={true}
          onOpenChange={() => undefined}
          workspaceSlug="acme"
          projectId={PROJECT}
          hideCycle={false}
          hideModule={false}
        />
      </QueryClientProvider>
    );
    expect(await screen.findByTestId("rich-filter-add")).toBeInTheDocument();
    expect(auditViewAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
    unmount();
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <WorkspaceViewDialog open={true} onOpenChange={() => undefined} workspaceSlug="acme" mode="create" />
      </QueryClientProvider>
    );
    expect(await screen.findByTestId("rich-filter-add")).toBeInTheDocument();
    expect(auditViewAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
  });
});
