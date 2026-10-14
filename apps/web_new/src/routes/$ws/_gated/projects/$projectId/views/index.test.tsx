// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Views tab home (SHELL-075). Thin mount: proves the route renders the
// Views area mount; the Views area epic owns the screen itself.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { Route } from "./index.js";

describe("views tab route", () => {
  it("renders the Views area mount", () => {
    const Component = Route.options.component;
    if (!Component) throw new Error("missing component");
    render(<Component />);
    expect(screen.getByRole("heading", { name: "Views" })).toBeInTheDocument();
  });
});
