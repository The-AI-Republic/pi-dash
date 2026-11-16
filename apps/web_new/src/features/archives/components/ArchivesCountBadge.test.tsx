// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { ArchivesCountBadge } from "./ArchivesCountBadge.js";

describe("ArchivesCountBadge", () => {
  it("hides entirely without a positive count", () => {
    const { unmount: unmountNull } = render(<ArchivesCountBadge count={null} />);
    expect(screen.queryByTestId("archives-count-badge")).toBeNull();
    unmountNull();

    const { unmount: unmountUndefined } = render(<ArchivesCountBadge count={undefined} />);
    expect(screen.queryByTestId("archives-count-badge")).toBeNull();
    unmountUndefined();

    render(<ArchivesCountBadge count={0} />);
    expect(screen.queryByTestId("archives-count-badge")).toBeNull();
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("shows the count with its hover tip", () => {
    render(<ArchivesCountBadge count={2} />);

    expect(screen.getByTestId("archives-count-badge")).toHaveTextContent("2");
    fireEvent.focus(screen.getByTestId("archives-count-badge"));
    const tip = screen.getByRole("tooltip");
    expect(tip).toHaveTextContent("2");
    expect(tip.textContent ?? "").toMatch(/work items?/);
  });

  it("uses the singular tip for one archived item", () => {
    render(<ArchivesCountBadge count={1} />);

    expect(screen.getByTestId("archives-count-badge")).toHaveTextContent("1");
    fireEvent.focus(screen.getByTestId("archives-count-badge"));
    expect(screen.getByRole("tooltip")).toHaveTextContent("1 archived work item");
  });
});
