// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// AI Dev Machines page (NEWFRONT-555, RUN-037–041). Proves the route passes
// the address's workspace slug into the dev-machines container.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

const container = vi.fn();
vi.mock("../../../../features/runners/devMachines/DevMachinesContainer.js", () => ({
  DevMachinesContainer: (props: { workspaceSlug: string }) => {
    container(props);
    return <div data-testid="dev-machines-container">{props.workspaceSlug}</div>;
  },
}));

import { Route } from "./index.js";

describe("ai-dev-machines route", () => {
  it("mounts the dev-machines container with the workspace slug", () => {
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme" });
    const Component = Route.options.component;
    if (!Component) throw new Error("missing component");
    render(<Component />);
    expect(screen.getByTestId("dev-machines-container")).toHaveTextContent("acme");
    expect(container).toHaveBeenCalledWith({ workspaceSlug: "acme" });
  });
});
