// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ListSkeleton } from "./ListSkeleton.js";

describe("ListSkeleton", () => {
  it("renders placeholder rows for the list", () => {
    render(<ListSkeleton />);
    expect(screen.getByTestId("issues-skeleton-list").querySelectorAll('[data-skeleton-row="true"]')).toHaveLength(8);
  });
});
