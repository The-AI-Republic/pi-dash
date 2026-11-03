// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { AssistantLayout } from "./AssistantLayout.js";

vi.mock("./ConversationRail.js", () => ({
  ConversationRail: (props: { workspaceSlug: string }) => <div data-testid="rail-slot">{props.workspaceSlug}</div>,
}));

describe("AssistantLayout", () => {
  it("mounts the rail beside the routed content", async () => {
    const rootRoute = createRootRoute();
    const layout = createRoute({
      getParentRoute: () => rootRoute,
      path: "/$ws",
      component: () => <AssistantLayout workspaceSlug="acme" />,
    });
    const child = createRoute({
      getParentRoute: () => layout,
      path: "/assistant",
      component: () => <p>routed</p>,
    });
    const router = createRouter({
      routeTree: rootRoute.addChildren([layout.addChildren([child])]),
      history: createMemoryHistory({ initialEntries: ["/acme/assistant"] }),
    });
    render(<RouterProvider router={router} />);
    expect(await screen.findByText("routed")).toBeInTheDocument();
    expect(screen.getByTestId("rail-slot")).toHaveTextContent("acme");
  });
});
