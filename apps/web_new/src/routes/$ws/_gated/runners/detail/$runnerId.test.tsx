// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner detail route, workspace scope (NEWFRONT-555, RUN-042/RUN-043).
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../../../../features/runners/detail/RunnerDetailContainer.js", () => ({
  RunnerDetailContainer: (props: { workspaceSlug: string; runnerId: string }) => (
    <div data-testid="runner-detail">{`${props.workspaceSlug}/${props.runnerId}`}</div>
  ),
}));

import { Route } from "./$runnerId.js";

describe("workspace runner detail route", () => {
  it("passes the slug and runner id through", () => {
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", runnerId: "r-1" });
    const Component = Route.options.component;
    if (!Component) throw new Error("missing component");
    render(<Component />);
    expect(screen.getByTestId("runner-detail")).toHaveTextContent("acme/r-1");
  });
});
