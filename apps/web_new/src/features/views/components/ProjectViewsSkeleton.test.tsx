// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { ProjectViewsSkeleton } from "./ProjectViewsSkeleton.js";

describe("ProjectViewsSkeleton", () => {
  it("renders row placeholders with no rows", () => {
    render(<ProjectViewsSkeleton />);
    const skeleton = screen.getByTestId("project-views-skeleton");
    expect(skeleton.querySelectorAll('[data-skeleton-row="true"]').length).toBeGreaterThan(0);
    expect(screen.queryByTestId("project-views-list")).not.toBeInTheDocument();
  });
});
