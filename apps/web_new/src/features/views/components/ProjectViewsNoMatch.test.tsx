// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { ProjectViewsNoMatch } from "./ProjectViewsNoMatch.js";

describe("ProjectViewsNoMatch", () => {
  it("suggests adjusting the search terms", () => {
    render(<ProjectViewsNoMatch />);
    expect(screen.getByTestId("project-views-no-match")).toBeInTheDocument();
    expect(screen.getByText("No matching results.")).toBeInTheDocument();
  });
});
