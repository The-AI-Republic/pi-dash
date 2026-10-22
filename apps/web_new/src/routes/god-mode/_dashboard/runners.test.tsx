// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// What this suite owns is the mount: the runners route renders the
// runners page. Table behavior belongs to the admin area.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../../features/admin/index.js", () => ({
  AdminRunnersPage: () => <div data-testid="runners-page" />,
}));

import { Route } from "./runners.js";

describe("runners route", () => {
  it("mounts the runners page", () => {
    const Component = Route.options.component;
    if (!Component) throw new Error("runners route has no component");
    render(<Component />);
    expect(screen.getByTestId("runners-page")).toBeInTheDocument();
  });
});
