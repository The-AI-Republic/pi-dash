// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Install-detail route composition (parity/specs/project-settings-part-f.md
// §PSET-059): the route hands its workspace, project and binding params to
// the feature-owned detail page.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../../../../features/schedulers/BindingDetailPage.js", () => ({
  BindingDetailPage: ({
    workspaceSlug,
    projectId,
    bindingId,
  }: {
    workspaceSlug: string;
    projectId: string;
    bindingId: string;
  }) => (
    <div
      data-testid="binding-detail"
      data-workspace={workspaceSlug}
      data-project={projectId}
      data-binding={bindingId}
    />
  ),
}));

import { Route } from "./$bindingId.js";

afterEach(() => {
  vi.restoreAllMocks();
});

describe("scheduler install detail route", () => {
  it("hands the route params to the detail page", () => {
    vi.spyOn(Route, "useParams").mockReturnValue({ ws: "acme", projectId: "p1", bindingId: "b1" });
    const Component = Route.options.component;
    if (!Component) throw new Error("scheduler install route has no component");
    render(<Component />);
    const body = screen.getByTestId("binding-detail");
    expect(body).toHaveAttribute("data-workspace", "acme");
    expect(body).toHaveAttribute("data-project", "p1");
    expect(body).toHaveAttribute("data-binding", "b1");
  });
});
