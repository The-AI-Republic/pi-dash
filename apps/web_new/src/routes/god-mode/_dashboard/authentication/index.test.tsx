// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// What this suite owns is the mount: the authentication address renders
// the landing page (its switches, cards, and skeleton belong to the
// admin area).
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../../../features/admin/components/AdminAuthLanding.js", () => ({
  AdminAuthLanding: () => <div data-testid="admin-auth-landing" />,
}));

import { Route } from "./index.js";

describe("authentication route", () => {
  it("renders the landing page", () => {
    const Component = Route.options.component;
    if (!Component) throw new Error("authentication route has no component");
    render(<Component />);
    expect(screen.getByTestId("admin-auth-landing")).toBeInTheDocument();
  });
});
