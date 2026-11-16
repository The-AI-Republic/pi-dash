// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Not-authorized view (NEWFRONT-540, RUN-001). The heading is the parity
// contract; the supporting sentence names the scope.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { RUNNERS_DENIED_HEADING, RunnersNotAuthorized } from "./NotAuthorizedView.js";

describe("RunnersNotAuthorized", () => {
  it("renders the shared heading with the workspace sentence", () => {
    render(<RunnersNotAuthorized scope="workspace" />);
    expect(screen.getByRole("heading", { name: RUNNERS_DENIED_HEADING })).toBeInTheDocument();
    expect(screen.getByText(/workspace admins and members can open AI Workers/)).toBeInTheDocument();
  });

  it("names the project for the project scope", () => {
    render(<RunnersNotAuthorized scope="project" />);
    expect(screen.getByText(/members of this project can open its AI Workers/)).toBeInTheDocument();
  });
});
