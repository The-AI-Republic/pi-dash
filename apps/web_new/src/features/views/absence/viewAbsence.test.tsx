// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Views absence audits (VIEW-047, from parity/specs/views.md §13): the
// auditor on synthetic fixtures, then every light views surface
// audited to zeros — lists with an open row menu, searches, switchers,
// the missing state and the confirm/publish dialogs. The save dialogs
// are audited beside their tab-order pins in
// dialog/viewDialogAbsence.test.tsx; the detail screens ride the node
// scan, which covers every source file.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  Link: ({ to, children, className }: { to: string; children?: React.ReactNode; className?: string }) => (
    <a href={to} className={className}>
      {children}
    </a>
  ),
  useNavigate: () => vi.fn(),
}));

const session = vi.hoisted(() => ({ useMe: vi.fn(), usePermissions: vi.fn() }));
vi.mock("../../../core/session/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../core/session/index.js")>()),
  useMe: session.useMe,
  usePermissions: session.usePermissions,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { auditViewAbsence } from "./viewAbsence.js";
import { makeMember, makeView, VIEWS_OWNER_ID } from "../testing.js";
import { DeleteViewDialog } from "../dialog/DeleteViewDialog.js";
import { PublishViewDialog } from "../dialog/PublishViewDialog.js";
import { SavedViewMissingState } from "../detail/SavedViewMissingState.js";
import { SavedViewSwitcher } from "../detail/SavedViewSwitcher.js";
import { WorkspaceViewSwitcher } from "../detail/WorkspaceViewSwitcher.js";
import { ProjectViewRowMenu } from "../components/ProjectViewRowMenu.js";
import { ProjectViewsListScreen } from "../components/ProjectViewsListScreen.js";
import { ProjectViewsSearch } from "../components/ProjectViewsSearch.js";
import { WorkspaceViewsListScreen } from "../components/WorkspaceViewsListScreen.js";
import { WorkspaceViewsSearch } from "../components/WorkspaceViewsSearch.js";

function fixture(html: string): ParentNode {
  const host = document.createElement("div");
  host.innerHTML = html;
  return host;
}

describe("auditViewAbsence", () => {
  it("reports zeros for ordinary controls", () => {
    const root = fixture(
      '<button type="button">Save changes</button>' +
        '<a href="/docs">Read the docs</a>' +
        '<input type="text" value="">' +
        '<button type="button" aria-label="">Retry</button>' +
        '<button type="button"></button>'
    );
    expect(auditViewAbsence(root)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
  });

  it("counts drag sources, file choosers, and transfer controls", () => {
    const root = fixture(
      '<div draggable="true">row</div>' +
        '<div draggable="false">pinned</div>' +
        '<input type="file">' +
        '<button type="button">Export CSV</button>' +
        '<a href="/import" aria-label="Import workspace">go</a>' +
        '<button type="button">Print summary</button>'
    );
    expect(auditViewAbsence(root)).toEqual({ draggable: 1, fileInputs: 1, transferControls: 3 });
  });

  it("ignores near-miss names without word boundaries", () => {
    const root = fixture(
      '<button type="button">Important notice</button>' +
        '<a href="/report">Exporter report</a>' +
        '<button type="button">Fingerprint settings</button>'
    );
    expect(auditViewAbsence(root)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
  });
});

const WS_ID = "123e4567-e89b-12d3-a456-426614174000";
const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";

function projectRow() {
  return {
    id: PROJECT_ID,
    name: "Website relaunch",
    identifier: "WEB",
    sort_order: null,
    logo_props: {},
    member_role: 15,
    archived_at: null,
    workspace: WS_ID,
    cycle_view: true,
    issue_views_view: true,
    module_view: true,
    page_view: false,
    inbox_view: false,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 0,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    agent_executor_options: [],
    default_agent_executor: "local_runner",
  };
}

interface Harness {
  views: ReturnType<typeof makeView>[];
  customs: ReturnType<typeof makeView>[];
}

let harness: Harness;

function jsonResponse(url: string, status: number, body: unknown) {
  return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
}

beforeEach(() => {
  harness = { views: [], customs: [] };
  session.useMe.mockReturnValue({ data: { id: VIEWS_OWNER_ID } });
  session.usePermissions.mockReturnValue({ isAdmin: false, isLoading: false });
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/auth/get-csrf-token/")) return jsonResponse(url, 200, { csrf_token: "csrf" });
    if (url.endsWith(`/api/workspaces/acme/projects/${PROJECT_ID}/`)) return jsonResponse(url, 200, projectRow());
    if (url.endsWith("/project-roles/")) return jsonResponse(url, 200, { [PROJECT_ID]: 20 });
    if (url.endsWith(`/api/workspaces/acme/projects/${PROJECT_ID}/views/`)) {
      return jsonResponse(url, 200, harness.views);
    }
    if (url.includes("/api/workspaces/acme/views/?")) return jsonResponse(url, 200, harness.customs);
    if (url.endsWith("/api/workspaces/acme/members/")) return jsonResponse(url, 200, [makeMember(VIEWS_OWNER_ID)]);
    if (url.endsWith("/api/users/me/")) return jsonResponse(url, 200, { id: VIEWS_OWNER_ID });
    if (url.endsWith("/api/users/me/workspaces/")) {
      return jsonResponse(url, 200, [{ id: WS_ID, name: "Acme", slug: "acme" }]);
    }
    throw new Error(`unexpected request: ${url}`);
  }) as never);
});

afterEach(() => {
  resetAppClient();
  document.title = "";
  session.useMe.mockReset();
  session.usePermissions.mockReset();
});

function renderProjectList(): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ProjectViewsListScreen workspaceSlug="acme" projectId={PROJECT_ID} />
    </QueryClientProvider>
  );
}

function renderWorkspaceList(): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <WorkspaceViewsListScreen workspaceSlug="acme" />
    </QueryClientProvider>
  );
}

describe("views list invariants", () => {
  it("audits the project list with an open row menu to zeros (VIEW-047)", async () => {
    harness.views = [makeView({ name: "V1 Star", owned_by: VIEWS_OWNER_ID })];
    renderProjectList();
    await waitFor(() => expect(screen.getByTestId("project-views-list")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Actions for V1 Star" }));
    await waitFor(() => expect(screen.getAllByRole("menuitem").length).toBeGreaterThan(0));
    expect(auditViewAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
  });

  it("audits the workspace list with an open row menu to zeros (VIEW-047)", async () => {
    harness.customs = [makeView({ name: "Alpha board" })];
    renderWorkspaceList();
    await screen.findByTestId("workspace-views-customs");
    fireEvent.click(screen.getByRole("button", { name: "Actions for Alpha board" }));
    await waitFor(() => expect(screen.getAllByRole("menuitem").length).toBeGreaterThan(0));
    expect(auditViewAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
  });

  it("offers no drag or transfer entry on the project row menu (VIEW-047)", () => {
    render(
      <ProjectViewRowMenu viewName="V1 Star" items={["edit", "open", "copy", "delete"]} onSelect={() => undefined} />
    );
    fireEvent.click(screen.getByRole("button", { name: "Actions for V1 Star" }));
    expect(auditViewAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
  });
});

describe("views search and switcher invariants", () => {
  it("audits the collapsed and expanded project search to zeros (VIEW-047)", () => {
    const { rerender } = render(<ProjectViewsSearch query="" onQueryChange={() => undefined} />);
    expect(auditViewAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
    fireEvent.click(screen.getByRole("button", { name: "Search views" }));
    expect(screen.getByPlaceholderText("Search")).toBeInTheDocument();
    expect(auditViewAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
    rerender(<ProjectViewsSearch query="tri" onQueryChange={() => undefined} />);
    expect(auditViewAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
  });

  it("audits the workspace search to zeros (VIEW-047)", () => {
    render(<WorkspaceViewsSearch query="tri" onQueryChange={() => undefined} />);
    expect(screen.getByPlaceholderText("Search")).toBeInTheDocument();
    expect(auditViewAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
  });

  it("audits both switchers shut to zeros (VIEW-047)", () => {
    const view = makeView({ name: "Triage" });
    const { unmount } = render(
      <SavedViewSwitcher current={view} views={[view]} loading={false} failed={false} onPick={() => undefined} />
    );
    expect(screen.getByRole("button", { name: "Triage" })).toBeInTheDocument();
    expect(auditViewAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
    unmount();
    render(
      <WorkspaceViewSwitcher
        currentKey={view.id}
        currentName={view.name}
        customs={[view]}
        loading={false}
        failed={false}
        onPick={() => undefined}
      />
    );
    expect(screen.getByRole("button", { name: "Triage" })).toBeInTheDocument();
    expect(auditViewAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
  });
});

describe("views dialog and empty-state invariants", () => {
  it("audits the missing-view state to zeros (VIEW-047)", () => {
    render(<SavedViewMissingState onBack={() => undefined} />);
    expect(screen.getByRole("heading", { name: /View does not exist/ })).toBeInTheDocument();
    expect(auditViewAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
  });

  it("audits the delete confirmation to zeros (VIEW-047)", () => {
    render(<DeleteViewDialog open={true} onCancel={() => undefined} onConfirm={() => undefined} />);
    expect(screen.getByRole("button", { name: "Delete" })).toBeInTheDocument();
    expect(auditViewAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
  });

  it("audits the publish dialog to zeros with and without a link (VIEW-047)", () => {
    const props = {
      viewName: "Triage",
      pending: false,
      onPublish: () => undefined,
      onUnpublish: () => undefined,
      onCopyLink: () => undefined,
      onClose: () => undefined,
    };
    const { unmount } = render(<PublishViewDialog open={true} link={null} {...props} />);
    expect(screen.getByRole("button", { name: "Publish" })).toBeInTheDocument();
    expect(auditViewAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
    unmount();
    render(<PublishViewDialog open={true} link="https://app.test/p/anchor-1" {...props} />);
    expect(screen.getByRole("button", { name: "Copy link" })).toBeInTheDocument();
    expect(auditViewAbsence(document)).toEqual({ draggable: 0, fileInputs: 0, transferControls: 0 });
  });
});
