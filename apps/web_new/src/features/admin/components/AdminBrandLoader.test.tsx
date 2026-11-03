// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { AdminBrandLoader } from "./AdminBrandLoader.js";

describe("AdminBrandLoader", () => {
  it("covers the screen with the spinner and no form beside it", () => {
    render(<AdminBrandLoader />);
    expect(screen.getByTestId("admin-loader")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Loading the admin console");
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
    expect(document.querySelector("form")).toBeNull();
  });
});
