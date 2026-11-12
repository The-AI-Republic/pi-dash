// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

// Stand-in for the page: the settings behavior belongs to the admin
// area. What this suite owns is the route wiring.
vi.mock("../../../features/admin/components/AdminEmailPage.js", () => ({
  AdminEmailPage: () => <div data-testid="admin-email-page" />,
}));

import { Route } from "./email.js";

describe("god-mode email route component", () => {
  it("renders the email settings page", () => {
    const Component = Route.options.component;
    if (!Component) throw new Error("god-mode email route has no component");
    render(<Component />);
    expect(screen.getByTestId("admin-email-page")).toBeInTheDocument();
  });
});
