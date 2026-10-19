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
// loop covers it after the chunk registers.
import { act, cleanup, render, screen } from "@testing-library/react";
import * as React from "react";
import type { RouteComponent } from "@tanstack/react-router";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import {
  registerWorkspaceSettingsPage,
  resetWorkspaceSettingsPages,
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

// The general section ships its own page (NEWFRONT-535); every other
// section still resolves through the shell's registration seam.
const SECTIONS: Array<[WorkspaceSettingsSectionKey, string, RouteComponent]> = [
  ["members", "members", MembersRoute.options.component as RouteComponent],
  ["exports", "exports", ExportsRoute.options.component as RouteComponent],
  ["integrations", "integrations", IntegrationsRoute.options.component as RouteComponent],
];

// The roster chunk registers its body when the route loads it; the test
// import does the same, and every test starts from that registration.
beforeEach(() => {
  registerMembersSettingsPage();
});

afterEach(() => {
  cleanup();
  resetWorkspaceSettingsPages();
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
