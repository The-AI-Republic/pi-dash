// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The detail badge counts the loaded items with the pinned tooltip,
// and stays absent on an empty module (MOD-029).

import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { ModuleDetailBadge } from "./ModuleDetailBadge.js";

describe("ModuleDetailBadge", () => {
  it("counts with the explanatory tooltip", () => {
    render(<ModuleDetailBadge total={2} />);
    const badge = screen.getByTestId("module-detail-badge");
    expect(badge).toHaveTextContent("2");
    fireEvent.focus(badge);
    expect(screen.getByRole("tooltip")).toHaveTextContent("There are 2 work items in this module");
  });

  it("uses the singular tooltip for one item", () => {
    render(<ModuleDetailBadge total={1} />);
    const badge = screen.getByTestId("module-detail-badge");
    expect(badge).toHaveTextContent("1");
    fireEvent.focus(badge);
    expect(screen.getByRole("tooltip")).toHaveTextContent("There are 1 work item in this module");
  });

  it("stays absent on an empty module", () => {
    const { container } = render(<ModuleDetailBadge total={0} />);
    expect(container).toBeEmptyDOMElement();
  });
});
