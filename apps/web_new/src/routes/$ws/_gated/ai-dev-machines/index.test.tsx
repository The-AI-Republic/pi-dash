// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// AI Dev Machines home (SHELL-054). Thin mount: proves the route
// renders the AI Dev Machines area mount; the dev-machines area epic
// owns the screen itself.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { Route } from "./index.js";

describe("ai-dev-machines route", () => {
  it("renders the AI Dev Machines area mount", () => {
    const Component = Route.options.component;
    if (!Component) throw new Error("missing component");
    render(<Component />);
    expect(screen.getByRole("heading", { name: "AI Dev Machines" })).toBeInTheDocument();
  });
});
