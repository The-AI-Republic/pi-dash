// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// What this suite owns is the mount: the general route renders the
// settings page itself. Guard plus shell belong to the dashboard
// layout route above it.
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../../features/admin/components/AdminGeneralSettingsPage.js", () => ({
  AdminGeneralSettingsPage: () => <div data-testid="admin-general-page" />,
}));

import { Route } from "./general.js";

describe("general settings route", () => {
  it("mounts the general settings page", () => {
    const Component = Route.options.component;
    if (!Component) throw new Error("general settings route has no component");
    render(<Component />);
    expect(screen.getByTestId("admin-general-page")).toBeInTheDocument();
  });
});
