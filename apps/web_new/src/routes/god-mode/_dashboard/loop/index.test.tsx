// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// What this suite owns is the mount: the loop route renders the jobs
// page. Table behavior belongs to the admin area.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../../../features/admin/components/AdminLoopJobsPage.js", () => ({
  AdminLoopJobsPage: () => <div data-testid="loop-jobs-page" />,
}));

import { Route } from "./index.js";

describe("loop jobs route", () => {
  it("mounts the jobs page", () => {
    const Component = Route.options.component;
    if (!Component) throw new Error("loop jobs route has no component");
    render(<Component />);
    expect(screen.getByTestId("loop-jobs-page")).toBeInTheDocument();
  });
});
