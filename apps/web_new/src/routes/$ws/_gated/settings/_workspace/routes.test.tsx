// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Section routes (WSET-001/036): every address mounts a thin outlet that
// asks the page registry for its own section body and hands it the
// workspace slug; the webhook detail address also hands over the id.
import { act, cleanup, render, screen } from "@testing-library/react";
import * as React from "react";
import type { RouteComponent } from "@tanstack/react-router";
import { afterEach, describe, expect, it } from "vitest";

import {
  registerWorkspaceSettingsPage,
  registerWorkspaceWebhookDetailPage,
  resetWorkspaceSettingsPages,
  type WorkspaceSettingsSectionKey,
} from "../../../../../features/workspace-settings/index.js";
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

const SECTIONS: Array<[WorkspaceSettingsSectionKey, string, RouteComponent]> = [
  ["general", "/", GeneralRoute.options.component as RouteComponent],
  ["members", "members", MembersRoute.options.component as RouteComponent],
  ["billing", "billing", BillingRoute.options.component as RouteComponent],
  ["exports", "exports", ExportsRoute.options.component as RouteComponent],
  ["webhooks", "webhooks", WebhooksRoute.options.component as RouteComponent],
  ["integrations", "integrations", IntegrationsRoute.options.component as RouteComponent],
];

afterEach(() => {
  cleanup();
  resetWorkspaceSettingsPages();
  resetRouteHarness();
  document.title = "";
});

describe("workspace settings section routes", () => {
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

  it("renders the registered webhook detail body with its id", async () => {
    const id = "223e4567-e89b-12d3-a456-426614174001";
    renderSettingsRoute({
      layoutComponent: LayoutComponent,
      component: WebhookDetailRoute.options.component as RouteComponent,
      leafPath: "webhooks/$webhookId",
      initialPath: `/acme/settings/webhooks/${id}`,
      transport: settingsTransport({ role: 20 }),
    });
    await act(async () => {
      registerWorkspaceWebhookDetailPage(({ workspaceSlug, webhookId }) => (
        <p>{`${workspaceSlug} detail ${webhookId}`}</p>
      ));
    });
    expect(await screen.findByText(`acme detail ${id}`)).toBeInTheDocument();
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
