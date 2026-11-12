// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The archived list skeleton: row shapes for the list region while
// the read is in flight.

import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { ArchivedCyclesSkeleton } from "./ArchivedCyclesSkeleton.js";

describe("ArchivedCyclesSkeleton", () => {
  it("renders placeholder rows under the skeleton handle", () => {
    render(<ArchivedCyclesSkeleton />);

    const skeleton = screen.getByTestId("archived-cycles-skeleton");
    expect(skeleton.querySelectorAll('[data-skeleton-row="true"]')).toHaveLength(4);
  });
});
