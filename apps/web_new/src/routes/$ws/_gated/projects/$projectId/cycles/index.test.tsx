// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycles tab home (NEWFRONT-707). Thin mount: proves the route renders
// the cycles list shell for the addressed project; the screen's states
// are covered in the feature suites.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../../../../../features/cycles/components/CyclesListScreen.js", () => ({
  CyclesListScreen: (props: Record<string, unknown>) => (
    <div
      data-testid="cycles-screen"
      data-workspace={String(props.workspaceSlug ?? "")}
      data-project={String(props.projectId ?? "")}
    />
  ),
}));

import { Route } from "./index.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";

describe("cycles tab route", () => {
  it("renders the cycles list shell for the project", () => {
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", projectId: PROJECT } as never);
    const Component = Route.options.component;
    if (!Component) throw new Error("missing component");
    render(
      <QueryClientProvider client={new QueryClient()}>
        <Component />
      </QueryClientProvider>
    );
    const node = screen.getByTestId("cycles-screen");
    expect(node).toHaveAttribute("data-workspace", "acme");
    expect(node).toHaveAttribute("data-project", PROJECT);
  });
});
