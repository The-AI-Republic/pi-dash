// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// General route wiring (NEWFRONT-535): the route hands the page a
// client-side navigate so the delete flow lands through the router, not a
// full page load.
import { cleanup, screen, waitFor } from "@testing-library/react";
import type { RouteComponent } from "@tanstack/react-router";
import { afterEach, describe, expect, it, vi } from "vitest";

const navigate = vi.hoisted(() => vi.fn(async () => undefined));

vi.mock("../../../../../features/workspace-settings/general/index.js", () => ({
  WorkspaceGeneralPage: ({
    workspaceSlug,
    onNavigate,
  }: {
    workspaceSlug: string;
    onNavigate?: (path: string) => void;
  }) => (
    <button type="button" onClick={() => onNavigate?.("/next/")}>
      {`general ${workspaceSlug}`}
    </button>
  ),
}));

import { Route as GeneralRoute } from "./index.js";
import { renderSettingsRoute, resetRouteHarness, settingsTransport } from "./-routeHarness.js";

const General = GeneralRoute.options.component;
if (General === undefined) throw new Error("missing general section component");

afterEach(() => {
  cleanup();
  resetRouteHarness();
  vi.clearAllMocks();
});

describe("general section route wiring", () => {
  it("hands the page the workspace slug and a client-side navigate", async () => {
    const view = renderSettingsRoute({
      component: General as RouteComponent,
      leafPath: "/",
      transport: settingsTransport({ role: 20 }),
    });
    vi.spyOn(view.router, "navigate").mockImplementation(navigate);
    const button = await screen.findByRole("button", { name: "general acme" });
    button.click();
    await waitFor(() => {
      expect(navigate).toHaveBeenCalledWith({ href: "/next/" });
    });
  });
});
