// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module row lead mark (parity/specs/modules.md §4.3, MOD-015): the
// lead's avatar when set, otherwise the neutral person glyph with the
// pinned no-lead explanation on hover.

import { fireEvent, render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { ModuleLeadMark } from "./ModuleLeadMark.js";

describe("ModuleLeadMark", () => {
  it("shows the lead avatar when one is set", () => {
    render(<ModuleLeadMark lead={{ name: "Ada Lovelace", avatarUrl: null }} testId="modules-row-lead" />);
    const lead = screen.getByTestId("modules-row-lead");
    expect(lead).toHaveAttribute("data-lead-kind", "avatar");
    expect(within(lead).getByText("AL")).toBeInTheDocument();
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("passes the directory image through when the lead has one", () => {
    render(
      <ModuleLeadMark
        lead={{ name: "Ada Lovelace", avatarUrl: "https://cdn.test/ada.png" }}
        testId="modules-row-lead"
      />
    );
    const lead = screen.getByTestId("modules-row-lead");
    expect(lead).toHaveAttribute("data-lead-kind", "avatar");
    expect(within(lead).getByRole("img", { name: "Ada Lovelace" })).toBe(lead.firstElementChild);
    expect(lead.querySelector("img")).toHaveAttribute("src", "https://cdn.test/ada.png");
  });

  it("marks no lead with the pinned explanation on hover", () => {
    render(<ModuleLeadMark lead={null} testId="modules-row-lead" />);
    const lead = screen.getByTestId("modules-row-lead");
    expect(lead).toHaveAttribute("data-lead-kind", "none");
    expect(within(lead).getByText("No lead", { selector: ".sr-only" })).toBeInTheDocument();
    fireEvent.focus(lead);
    expect(screen.getByRole("tooltip")).toHaveTextContent("No lead");
  });
});
