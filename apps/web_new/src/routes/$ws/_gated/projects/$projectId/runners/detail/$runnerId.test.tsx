// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner detail route, project scope (NEWFRONT-555, RUN-042/RUN-043).
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../../../../../../features/runners/detail/RunnerDetailContainer.js", () => ({
  RunnerDetailContainer: (props: { workspaceSlug: string; projectId?: string; runnerId: string }) => (
    <div data-testid="runner-detail">{`${props.workspaceSlug}/${props.projectId}/${props.runnerId}`}</div>
  ),
}));

import { Route } from "./$runnerId.js";

describe("project runner detail route", () => {
  it("passes the slug, project and runner id through", () => {
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", projectId: "prj-1", runnerId: "r-1" });
    const Component = Route.options.component;
    if (!Component) throw new Error("missing component");
    render(<Component />);
    expect(screen.getByTestId("runner-detail")).toHaveTextContent("acme/prj-1/r-1");
  });
});
