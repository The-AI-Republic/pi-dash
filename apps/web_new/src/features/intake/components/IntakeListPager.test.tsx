// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { INTAKE_PAGE_SKELETON_COUNT, IntakeListPager } from "./IntakeListPager.js";

describe("IntakeListPager (INT-007)", () => {
  it("renders placeholders over the sentinel while a page exists", () => {
    render(<IntakeListPager hasNextPage />);
    expect(screen.getAllByTestId("intake-page-skeleton")).toHaveLength(INTAKE_PAGE_SKELETON_COUNT);
    expect(screen.getByTestId("intake-pager-sentinel")).toBeInTheDocument();
  });

  it("leaves the sentinel empty once exhausted", () => {
    render(<IntakeListPager hasNextPage={false} />);
    expect(screen.queryByTestId("intake-page-skeleton")).not.toBeInTheDocument();
    expect(screen.getByTestId("intake-pager-sentinel")).toBeInTheDocument();
  });
});
