// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages tab route: the search parser owns the shareable tab value and the
// route passes it into the screen.

import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

const screenSpy = vi.hoisted(() => ({ render: vi.fn() }));

vi.mock("@tanstack/react-router", () => ({
  createFileRoute: () => (options: unknown) => ({
    options,
    useParams: () => ({ ws: "acme", projectId: "project-1" }),
    useSearch: () => ({ type: "private" }),
  }),
}));

vi.mock("../../../../../../features/pages/components/PagesListScreen.js", () => ({
  PagesListScreen: (props: unknown) => {
    screenSpy.render(props);
    return <div data-testid="pages-screen" />;
  },
}));

import { Route } from "./index.js";

describe("pages route", () => {
  it("parses the one supported search key", () => {
    const validate = Route.options.validateSearch as unknown as (input: Record<string, unknown>) => unknown;
    expect(validate({})).toEqual({ type: "public" });
    expect(validate({ type: "archived" })).toEqual({ type: "archived" });
    expect(validate({ type: "bogus" })).toEqual({ type: "public" });
  });

  it("passes the params and active tab to the screen", () => {
    const Component = Route.options.component;
    if (!Component) throw new Error("missing component");
    render(<Component />);
    expect(screen.getByTestId("pages-screen")).toBeInTheDocument();
    expect(screenSpy.render).toHaveBeenCalledWith({
      workspaceSlug: "acme",
      projectId: "project-1",
      tab: "private",
    });
  });
});
