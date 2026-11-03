// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The loading skeleton stands in the header, hero slot and row shapes
// (CYC-012).

import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { CyclesListSkeleton } from "./CyclesListSkeleton.js";

describe("CyclesListSkeleton", () => {
  it("renders the header, hero slot and row shapes", () => {
    render(<CyclesListSkeleton />);
    const skeleton = screen.getByTestId("cycles-list-skeleton");
    expect(skeleton).toBeInTheDocument();
    expect(skeleton.querySelector("[data-skeleton-hero]")).not.toBe(null);
    expect(skeleton.querySelectorAll("[data-skeleton-row]").length).toBeGreaterThan(0);
  });
});
