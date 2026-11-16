// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Overdue badge (ISS-236): nothing in the community build, the reading in
// a cloud build, and nothing for a row that is not overdue.
import { cleanup, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { edition } from "../../../core/edition/index.js";
import { DETAIL_BADGES_FLAG } from "./flags.js";
import { OverdueBadge } from "./OverdueBadge.js";

afterEach(() => {
  cleanup();
  delete edition.flags[DETAIL_BADGES_FLAG];
});

function enable(): void {
  edition.flags[DETAIL_BADGES_FLAG] = true;
}

describe("OverdueBadge", () => {
  it("renders nothing in the community build, even when the row is overdue", () => {
    render(<OverdueBadge dueDay="2020-05-05" today="2026-10-24" overdue />);
    expect(screen.queryByTestId("detail-overdue-badge")).toBeNull();
  });

  it("reads the days late in a cloud build", () => {
    enable();
    render(<OverdueBadge dueDay="2026-10-20" today="2026-10-24" overdue />);
    expect(screen.getByTestId("detail-overdue-badge")).toHaveTextContent("4 days overdue");
  });

  it("names a due-today item", () => {
    enable();
    render(<OverdueBadge dueDay="2026-10-24" today="2026-10-24" overdue />);
    expect(screen.getByTestId("detail-overdue-badge")).toHaveTextContent("Due today");
  });

  it("stays away for a row that is not overdue", () => {
    enable();
    render(<OverdueBadge dueDay="2026-10-24" today="2026-10-24" overdue={false} />);
    expect(screen.queryByTestId("detail-overdue-badge")).toBeNull();
  });

  it("renders nothing for a future due day", () => {
    enable();
    render(<OverdueBadge dueDay="2027-01-01" today="2026-10-24" overdue />);
    expect(screen.queryByTestId("detail-overdue-badge")).toBeNull();
  });
});
