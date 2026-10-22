// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Settings layout wiring (WSET-001/002/007/036): role-gated access to
// the frame, denial on the same address with no sidebar and no title,
// the workspace identity fallback chain, and client-side entry picks.
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import type { RouteComponent } from "@tanstack/react-router";
import { afterEach, describe, expect, it } from "vitest";

import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER } from "../../../../../core/session/index.js";
import { resetWorkspaceSettingsPages } from "../../../../../features/workspace-settings/index.js";
import { renderSettingsRoute, resetRouteHarness, settingsTransport } from "./-routeHarness.js";
import { Route } from "./route.js";

const Layout = Route.options.component;
if (Layout === undefined) throw new Error("missing settings layout component");
const LayoutComponent: RouteComponent = Layout;

function renderLayout(
  options: Parameters<typeof settingsTransport>[0],
  initialPath?: string,
  leaf: { leafPath: string; component: RouteComponent } = {
    leafPath: "/",
    component: () => <p>section body</p>,
  }
) {
  return renderSettingsRoute({
    layoutComponent: LayoutComponent,
    component: leaf.component,
    leafPath: leaf.leafPath,
    ...(initialPath === undefined ? {} : { initialPath }),
    transport: settingsTransport(options),
  });
}

afterEach(() => {
  cleanup();
  resetWorkspaceSettingsPages();
  resetRouteHarness();
  document.title = "";
});

describe("workspace settings layout", () => {
  it("renders the shell and the section outlet for admins", async () => {
    renderLayout({ role: ROLE_ADMIN });
    expect(await screen.findByTestId("workspace-settings-shell")).toBeInTheDocument();
    expect(screen.getByText("section body")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Administration" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Developer" })).toBeInTheDocument();
    await waitFor(() => {
      expect(document.title).toBe("Acme - General settings");
    });
  });

  it("uses the details read for the workspace name", async () => {
    renderLayout({ role: ROLE_ADMIN, detailsName: "Acme Renamed" });
    await screen.findByText("Acme Renamed");
    expect(document.title).toBe("Acme Renamed - General settings");
  });

  it("falls back to the listed workspace name and logo while details fail", async () => {
    renderLayout({ role: ROLE_ADMIN, detailsStatus: 500, listedLogoUrl: "https://cdn.test/listed.png" });
    expect(await screen.findByText("Acme Listed")).toBeInTheDocument();
    expect(screen.getByTestId("workspace-logo").querySelector("img")).toHaveAttribute(
      "src",
      "https://cdn.test/listed.png"
    );
  });

  it("prefers the details logo over the listed one", async () => {
    renderLayout({
      role: ROLE_ADMIN,
      detailsLogoUrl: "https://cdn.test/details.png",
      listedLogoUrl: "https://cdn.test/listed.png",
    });
    await waitFor(() => {
      expect(screen.getByTestId("workspace-logo").querySelector("img")).toHaveAttribute(
        "src",
        "https://cdn.test/details.png"
      );
    });
  });

  it("filters the frame for members", async () => {
    renderLayout({ role: ROLE_MEMBER });
    expect(await screen.findByRole("heading", { name: "Administration" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Developer" })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Billing & Plans" })).not.toBeInTheDocument();
  });

  it("denies admin-only sections to members on the same address", async () => {
    const { router } = renderLayout({ role: ROLE_MEMBER }, "/acme/settings/billing/", {
      leafPath: "billing",
      component: () => <p>billing body</p>,
    });
    expect(await screen.findByTestId("workspace-settings-not-authorized")).toBeInTheDocument();
    expect(screen.queryByTestId("workspace-settings-shell")).not.toBeInTheDocument();
    expect(screen.queryByText("section body")).not.toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/acme/settings/billing/");
    expect(document.title).toBe("");
  });

  it("denies every section to guests", async () => {
    renderLayout({ role: ROLE_GUEST });
    expect(await screen.findByTestId("workspace-settings-not-authorized")).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Members" })).not.toBeInTheDocument();
    expect(document.title).toBe("");
  });

  it("does not read the workspace details for a refused visitor", async () => {
    const requests: string[] = [];
    renderLayout({ role: ROLE_GUEST, onRequest: (url) => requests.push(url) });
    await screen.findByTestId("workspace-settings-not-authorized");
    expect(requests.some((url) => url.endsWith("/api/workspaces/acme/"))).toBe(false);
  });

  it("treats an unknown section name as refused instead of a dead frame", async () => {
    const { router } = renderLayout({ role: ROLE_ADMIN }, "/acme/settings/nope/", {
      leafPath: "$",
      component: () => null,
    });
    expect(await screen.findByTestId("workspace-settings-not-authorized")).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/acme/settings/nope/");
  });

  it("leaves the project-settings tabs to their own slice instead of denying them", async () => {
    const { router } = renderLayout({ role: ROLE_ADMIN }, "/acme/settings/projects/p1/labels/", {
      leafPath: "$",
      component: () => <p>labels body</p>,
    });
    expect(await screen.findByText("labels body")).toBeInTheDocument();
    expect(router.state.location.pathname).toBe("/acme/settings/projects/p1/labels/");
    expect(screen.queryByTestId("workspace-settings-not-authorized")).not.toBeInTheDocument();
    expect(screen.queryByTestId("workspace-settings-shell")).not.toBeInTheDocument();
    expect(document.title).not.toContain("General settings");
  });

  it("shows the ordinary not-found page for a generic project-settings address", async () => {
    renderLayout({ role: ROLE_ADMIN }, "/acme/settings/projects/", {
      leafPath: "$",
      component: () => null,
    });
    expect(await screen.findByText("That page does not exist")).toBeInTheDocument();
    expect(screen.queryByTestId("workspace-settings-not-authorized")).not.toBeInTheDocument();
  });

  it("shows the ordinary not-found page for an address below a project tab", async () => {
    // EST-026: a guessed estimate-system id is not part of any tab, so
    // the layout renders not-found instead of an empty outlet.
    renderLayout({ role: ROLE_ADMIN }, "/acme/settings/projects/p1/estimates/e9/", {
      leafPath: "$",
      component: () => null,
    });
    expect(await screen.findByText("That page does not exist")).toBeInTheDocument();
    expect(screen.queryByTestId("workspace-settings-not-authorized")).not.toBeInTheDocument();
    expect(screen.queryByTestId("workspace-settings-shell")).not.toBeInTheDocument();
  });

  it("enters the webhook detail frame with its own title", async () => {
    renderLayout({ role: ROLE_ADMIN }, "/acme/settings/webhooks/223e4567-e89b-12d3-a456-426614174001", {
      leafPath: "webhooks/$webhookId",
      component: () => <p>webhook body</p>,
    });
    expect(await screen.findByTestId("workspace-settings-shell")).toBeInTheDocument();
    expect(screen.getByText("webhook body")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Webhook", level: 1 })).toBeInTheDocument();
    expect(screen.getByTestId("workspace-settings-mobile-crumb")).toHaveTextContent("Webhook");
    await waitFor(() => {
      expect(document.title).toBe("Acme - Webhook");
    });
  });

  it("navigates between sections in-app", async () => {
    const { router } = renderLayout({ role: ROLE_ADMIN });
    fireEvent.click(await screen.findByRole("link", { name: "Members" }));
    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/acme/settings/members");
    });
  });

  it("returns to the workspace home from the identity back control", async () => {
    const { router } = renderLayout({ role: ROLE_ADMIN });
    fireEvent.click(await screen.findByRole("link", { name: /Back to workspace/ }));
    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/acme");
    });
  });

  it("holds the frame until the viewer's role resolves", () => {
    renderLayout({ role: ROLE_ADMIN, workspacesPending: true });
    expect(screen.queryByTestId("workspace-settings-shell")).not.toBeInTheDocument();
    expect(screen.queryByTestId("workspace-settings-not-authorized")).not.toBeInTheDocument();
  });
});
