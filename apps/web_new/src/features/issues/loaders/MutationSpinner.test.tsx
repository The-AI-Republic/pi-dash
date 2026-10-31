// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { MutationSpinner } from "./MutationSpinner.js";

describe("MutationSpinner", () => {
  it("floats a status spinner docked top-right", () => {
    render(<MutationSpinner />);
    const box = screen.getByTestId("issues-mutation-spinner");
    expect(box).toHaveClass("fixed");
    expect(box.className).toContain("top-");
    expect(box.className).toContain("right-");
    expect(screen.getByRole("status")).toBeInTheDocument();
    expect(screen.getByText("Updating issues")).toBeInTheDocument();
  });
});
