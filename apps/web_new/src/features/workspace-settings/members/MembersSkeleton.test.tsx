// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Roster skeleton (WSET-014): a status region with one placeholder per row.
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { MembersSkeleton, ROSTER_SKELETON_ROWS } from "./MembersSkeleton.js";

describe("MembersSkeleton", () => {
  it("announces its status and paints the placeholder rows", () => {
    const { container } = render(<MembersSkeleton />);
    expect(screen.getByRole("status", { name: "Loading members" })).toBeInTheDocument();
    expect(screen.getByText("Loading members")).toHaveClass("sr-only");
    expect(container.querySelectorAll("span[style]")).toHaveLength(ROSTER_SKELETON_ROWS * 4);
  });
});
