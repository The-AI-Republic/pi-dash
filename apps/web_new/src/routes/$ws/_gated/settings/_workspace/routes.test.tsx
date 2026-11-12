// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Section routes (WSET-001/019/022/036): the unclaimed addresses mount a
// thin outlet that asks the page registry for their section body; the
// webhooks half (NEWFRONT-541) mounts its own list and detail pages in the
// same shell, with the workspace slug and the webhook id, and the billing
// half (NEWFRONT-546) likewise mounts its own page. The members roster
// (NEWFRONT-537) registers its body from its own chunk, so the registry
// loop covers it after the chunk registers, and exports (NEWFRONT-548)
// registers its entry page through the same seam.
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import type { RouteComponent } from "@tanstack/react-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Transport } from "@pidash/api-client";
import {
  ExportsSettingsPage,
  getWorkspaceExportsGuide,
  getWorkspaceSettingsPage,
  registerWorkspaceSettingsPage,
  resetWorkspaceExportsGuide,
  resetWorkspaceSettingsPages,
  type WorkspaceSettingsPageProps,
  type WorkspaceSettingsSectionKey,
} from "../../../../../features/workspace-settings/index.js";
import { registerMembersSettingsPage } from "../../../../../features/workspace-settings/members/index.js";
import { Route as BillingRoute } from "./billing/index.js";
import { Route as ExportsRoute } from "./exports/index.js";
import { Route as GeneralRoute } from "./index.js";
import { Route as IntegrationsRoute } from "./integrations/index.js";
import { Route as MembersRoute } from "./members/index.js";
import { Route as WebhookDetailRoute } from "./webhooks/$webhookId.js";
import { Route as WebhooksRoute } from "./webhooks/index.js";
import { Route as UnknownSettingsRoute } from "./$.js";

import { renderSettingsRoute, resetRouteHarness, settingsTransport } from "./-routeHarness.js";
import { Route as LayoutRoute } from "./route.js";

const Layout = LayoutRoute.options.component;
if (Layout === undefined) throw new Error("missing settings layout component");
const LayoutComponent: RouteComponent = Layout;

// The general, integrations and exports sections ship their own pages
// (NEWFRONT-535, -539, -548); members still resolves through the shell's
// registration seam.

/** Body that reports a completed leave the way the roster page does. */
function LeaveProbe({ onLeaveWorkspace }: WorkspaceSettingsPageProps): React.ReactElement {
  return (
    <button type="button" onClick={onLeaveWorkspace}>
      Leave for the test
    </button>
  );
}

const SECTIONS: Array<[WorkspaceSettingsSectionKey, string, RouteComponent]> = [
  ["members", "members", MembersRoute.options.component as RouteComponent],
];

const EXPORTS_USER_ID = "11111111-1111-4111-8111-111111111111";

function exportsProjectRow(id: string, name: string, identifier: string) {
  return {
    id,
    name,
    identifier,
    member_role: 15,
    archived_at: null,
    workspace: "44444444-4444-4444-8444-444444444444",
    created_at: "2026-01-01T00:00:00.000Z",
    updated_at: "2026-01-01T00:00:00.000Z",
  };
}

function exportsHistoryPage() {
  const stamped = new Date(Date.now() - 3_600_000).toISOString();
  return {
    grouped_by: null,
    sub_grouped_by: null,
    total_count: 1,
    next_cursor: "10:1:0",
    prev_cursor: "10:-1:1",
    next_page_results: false,
    prev_page_results: false,
    count: 1,
    total_pages: 1,
    total_results: 1,
    extra_stats: null,
    results: [
      {
        id: "12345678-1234-4234-8234-000000000001",
        created_at: stamped,
        updated_at: stamped,
        project: ["12345678-1234-4234-8234-000000000901"],
        provider: "csv",
        status: "completed",
        url: "https://files.test/export-1.csv",
        initiated_by: EXPORTS_USER_ID,
        initiated_by_detail: {
          id: EXPORTS_USER_ID,
          first_name: "Ana",
          last_name: "Siva",
          avatar: "",
          avatar_url: null,
          display_name: "Ana Siva",
          is_bot: false,
        },
        token: "token",
        created_by: null,
        updated_by: null,
      },
    ],
  };
}

function exportsAnswer(url: string, body: unknown) {
  return {
    status: 200,
    url,
    headers: { get: () => "application/json" },
    text: async () => JSON.stringify(body),
  };
}

/** Settings frame reads plus the project and history reads the guide makes. */
function exportsTransport(role: number): Transport {
  const frame = settingsTransport({ role });
  return async (url, init) => {
    if (url.endsWith("/projects/")) {
      return exportsAnswer(url, [
        exportsProjectRow("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "Exp Alpha", "EA"),
        exportsProjectRow("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", "Exp Beta", "EB"),
      ]);
    }
    if (url.includes("/export-issues")) return exportsAnswer(url, exportsHistoryPage());
    return frame(url, init);
  };
}

// The roster chunk registers its body when the route loads it; the test
// import does the same, and every test starts from that registration.
beforeEach(() => {
  registerMembersSettingsPage();
});

afterEach(() => {
  cleanup();
  resetWorkspaceSettingsPages();
  resetWorkspaceExportsGuide();
  resetRouteHarness();
  document.title = "";
});

describe("workspace settings section routes", () => {
  it("renders the general page from its own route", async () => {
    renderSettingsRoute({
      layoutComponent: LayoutComponent,
      component: GeneralRoute.options.component as RouteComponent,
      leafPath: "/",
      transport: settingsTransport({ role: 20 }),
    });
    expect(await screen.findByTestId("workspace-general-identity")).toBeInTheDocument();
  });

  for (const [section, leafPath, component] of SECTIONS) {
    it(`renders the registered ${section} body for its own address`, async () => {
      await act(async () => {
        registerWorkspaceSettingsPage(section, ({ workspaceSlug }) => <p>{`${section} of ${workspaceSlug}`}</p>);
      });
      renderSettingsRoute({
        layoutComponent: LayoutComponent,
        component,
        leafPath,
        transport: settingsTransport({ role: 20 }),
      });
      expect(await screen.findByText(`${section} of acme`)).toBeInTheDocument();
    });

    it(`leaves the ${section} frame quiet while its body is unregistered`, async () => {
      resetWorkspaceSettingsPages();
      renderSettingsRoute({
        layoutComponent: LayoutComponent,
        component,
        leafPath,
        transport: settingsTransport({ role: 20 }),
      });
      expect(await screen.findByTestId("workspace-settings-shell")).toBeInTheDocument();
      expect(await screen.findByTestId("workspace-settings-section-placeholder")).toBeInTheDocument();
    });
  }

  // The billing section (NEWFRONT-546) owns its page body too: admins get
  // the static plans page inside the frame, and members are refused by the
  // shell's admin gate on the same address.
  it("mounts the billing plans page for its own address", async () => {
    renderSettingsRoute({
      layoutComponent: LayoutComponent,
      component: BillingRoute.options.component as RouteComponent,
      leafPath: "billing",
      transport: settingsTransport({ role: 20 }),
    });
    expect(await screen.findByTestId("workspace-settings-shell")).toBeInTheDocument();
    expect(await screen.findByTestId("workspace-settings-billing")).toBeInTheDocument();
    expect(screen.queryByTestId("workspace-settings-section-placeholder")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "All plans" })).toBeInTheDocument();
  });

  it("refuses a member on the billing address without rendering the page", async () => {
    const requested: string[] = [];
    renderSettingsRoute({
      layoutComponent: LayoutComponent,
      component: BillingRoute.options.component as RouteComponent,
      leafPath: "billing",
      transport: settingsTransport({ role: 15, onRequest: (url) => requested.push(url) }),
    });
    expect(await screen.findByTestId("workspace-settings-not-authorized")).toBeInTheDocument();
    expect(screen.queryByTestId("workspace-settings-billing")).not.toBeInTheDocument();
    expect(screen.queryByTestId("workspace-settings-shell")).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "All plans" })).not.toBeInTheDocument();
  });

  // The members chunk registers its body when its route imports it, as the
  // browser loads it; the test import does the same, and each test starts
  // from that registration. The reads answer 404 here, so the failed state
  // proves the real roster mounted with the workspace slug.
  it("mounts the roster body the members chunk registers", async () => {
    const requested: string[] = [];
    renderSettingsRoute({
      layoutComponent: LayoutComponent,
      component: MembersRoute.options.component as RouteComponent,
      leafPath: "members",
      transport: settingsTransport({ role: 20, onRequest: (url) => requested.push(url) }),
    });
    expect(await screen.findByTestId("workspace-settings-shell")).toBeInTheDocument();
    expect(await screen.findByTestId("workspace-settings-members")).toBeInTheDocument();
    expect(await screen.findByTestId("members-load-failed")).toBeInTheDocument();
    expect(requested.some((url) => url.endsWith("/api/workspaces/acme/members/"))).toBe(true);
    expect(requested.some((url) => url.endsWith("/api/workspaces/acme/invitations/"))).toBe(true);
  });

  it("renders the integrations page from its own route", async () => {
    renderSettingsRoute({
      layoutComponent: LayoutComponent,
      component: IntegrationsRoute.options.component as RouteComponent,
      leafPath: "integrations",
      transport: settingsTransport({ role: 20 }),
    });
    expect(await screen.findByTestId("workspace-settings-integrations")).toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: "GitHub" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "GitLab" })).toBeInTheDocument();
  });

  // WSET-016: the integrations address is admin-only. The shell owns the
  // gate, so a member never reaches the page body or its provider reads.
  it("refuses a member on the integrations address without rendering the page", async () => {
    const requested: string[] = [];
    renderSettingsRoute({
      layoutComponent: LayoutComponent,
      component: IntegrationsRoute.options.component as RouteComponent,
      leafPath: "integrations",
      transport: settingsTransport({ role: 15, onRequest: (url) => requested.push(url) }),
    });
    expect(await screen.findByTestId("workspace-settings-not-authorized")).toBeInTheDocument();
    expect(screen.queryByTestId("workspace-settings-integrations")).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "GitHub" })).not.toBeInTheDocument();
    expect(requested.some((url) => url.includes("/integrations/github/"))).toBe(false);
    expect(requested.some((url) => url.includes("/integrations/git/accounts/"))).toBe(false);
  });

  // §13: a successful leave hands navigation to the shell, which lands on
  // the just-left workspace home (and its gate) rather than the picker. The
  // oracle polls for the slash-terminated address.
  it("sends a leaver to the just-left workspace home", async () => {
    registerWorkspaceSettingsPage("members", LeaveProbe);
    const view = renderSettingsRoute({
      layoutComponent: LayoutComponent,
      component: MembersRoute.options.component as RouteComponent,
      leafPath: "members",
      // The app router keeps authored trailing slashes (`main.tsx`).
      trailingSlash: "preserve",
      transport: settingsTransport({ role: 20 }),
    });
    const button = await screen.findByRole("button", { name: "Leave for the test" });
    await userEvent.click(button);
    await vi.waitFor(() => expect(view.router.state.location.pathname).toBe("/acme/"));
  });

  it("mounts the webhooks list page for its own address", async () => {
    renderSettingsRoute({
      layoutComponent: LayoutComponent,
      component: WebhooksRoute.options.component as RouteComponent,
      leafPath: "webhooks",
      transport: settingsTransport({ role: 20 }),
    });
    expect(await screen.findByTestId("workspace-settings-shell")).toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: "Webhooks", level: 2 })).toBeInTheDocument();
    // The frame's stub answers 404 for the list: the page owns that state
    // and still renders its heading and Add control.
    expect(await screen.findByRole("button", { name: "Add webhook" })).toBeInTheDocument();
  });

  // Exports (NEWFRONT-548) publishes its entry into the shell's page
  // registry while the section route is mounted, so the settings shell
  // keeps one render path for every section body.
  it("registers the exports entry with the shell page registry", async () => {
    expect(getWorkspaceSettingsPage("exports")).toBeNull();
    renderSettingsRoute({
      layoutComponent: LayoutComponent,
      component: ExportsRoute.options.component as RouteComponent,
      leafPath: "exports",
      transport: settingsTransport({ role: 20 }),
    });
    expect(await screen.findByTestId("workspace-exports-settings")).toBeInTheDocument();
    expect(getWorkspaceSettingsPage("exports")).toBe(ExportsSettingsPage);
  });

  // Exports (NEWFRONT-768): the route lazy-loads the guide chunk, so the
  // frame's seam fills with the real form + history for admitted roles.
  // Nothing here registers the guide by hand: if the route stops loading
  // the chunk, the pending slot stays and this fails.
  it("loads the exports guide through the route for a member", async () => {
    expect(getWorkspaceExportsGuide()).toBeNull();
    renderSettingsRoute({
      layoutComponent: LayoutComponent,
      component: ExportsRoute.options.component as RouteComponent,
      leafPath: "exports",
      transport: exportsTransport(15),
    });
    expect(await screen.findByTestId("workspace-exports-settings")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 2, name: "Exports" })).toBeInTheDocument();
    expect(
      screen.getByText(
        "Export your project data in various formats and access your export history with download links."
      )
    ).toBeInTheDocument();
    expect(await screen.findByRole("heading", { level: 3, name: "Exporting project" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Export" })).toBeInTheDocument();
    expect(await screen.findByRole("heading", { level: 3, name: "Previous exports" })).toBeInTheDocument();
    expect(await screen.findByText("Ana Siva")).toBeInTheDocument();
    expect(screen.queryByTestId("workspace-exports-guide-pending")).not.toBeInTheDocument();
    expect(getWorkspaceExportsGuide()).not.toBeNull();
  });

  it("denies a guest on the exports address without leaving it", async () => {
    const view = renderSettingsRoute({
      layoutComponent: LayoutComponent,
      component: ExportsRoute.options.component as RouteComponent,
      leafPath: "exports",
      transport: settingsTransport({ role: 5 }),
    });
    expect(await screen.findByTestId("workspace-settings-not-authorized")).toBeInTheDocument();
    expect(view.router.state.location.pathname).toBe("/acme/settings/exports/");
    expect(screen.queryByRole("heading", { level: 3, name: "Exporting project" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { level: 3, name: "Previous exports" })).not.toBeInTheDocument();
  });

  it("mounts the webhook detail page for its id", async () => {
    const id = "223e4567-e89b-12d3-a456-426614174001";
    renderSettingsRoute({
      layoutComponent: LayoutComponent,
      component: WebhookDetailRoute.options.component as RouteComponent,
      leafPath: "webhooks/$webhookId",
      initialPath: `/acme/settings/webhooks/${id}`,
      transport: async (url, init) => {
        if (url.endsWith(`/webhooks/${id}/`)) {
          return {
            status: 200,
            url,
            headers: { get: () => null },
            text: async () =>
              JSON.stringify({
                id,
                url: "https://example.com/post",
                is_active: true,
                created_at: "2026-01-01T00:00:00Z",
                updated_at: "2026-01-01T00:00:00Z",
                project: true,
                issue: true,
                cycle: true,
                module: true,
                issue_comment: true,
              }),
          };
        }
        return settingsTransport({ role: 20 })(url, init);
      },
    });
    expect(await screen.findByText("Webhook details")).toBeInTheDocument();
    expect(await screen.findByDisplayValue("https://example.com/post")).toBeInTheDocument();
  });

  it("explains an unknown webhook id instead of spinning", async () => {
    renderSettingsRoute({
      layoutComponent: LayoutComponent,
      component: WebhookDetailRoute.options.component as RouteComponent,
      leafPath: "webhooks/$webhookId",
      initialPath: "/acme/settings/webhooks/00000000-0000-0000-0000-000000000000",
      // The frame's own reads still answer; only the detail id is unknown.
      transport: async (url, init) => {
        if (!url.endsWith("/auth/get-csrf-token/") && url.includes("/webhooks/")) {
          return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
        }
        return settingsTransport({ role: 20 })(url, init);
      },
    });
    expect(await screen.findByTestId("workspace-settings-webhook-missing")).toBeInTheDocument();
  });

  it("renders no body from the catch-all leaf that keeps unknown addresses inside the frame", () => {
    const Component = UnknownSettingsRoute.options.component;
    if (Component === undefined) throw new Error("missing unknown-settings leaf component");
    const { container } = render(<Component />);
    expect(container).toBeEmptyDOMElement();
  });

  it("keeps the unknown-settings address bodyless inside the frame", async () => {
    renderSettingsRoute({
      layoutComponent: LayoutComponent,
      component: () => null,
      leafPath: "$",
      initialPath: "/acme/settings/unknown/",
      transport: settingsTransport({ role: 20 }),
    });
    // The layout refuses an unknown section before any outlet body shows.
    expect(await screen.findByTestId("workspace-settings-not-authorized")).toBeInTheDocument();
  });
});
