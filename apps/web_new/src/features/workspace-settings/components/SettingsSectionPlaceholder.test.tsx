// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unregistered-section placeholder (WSET-001/039): an honest note, with
// no controls, drag targets or file inputs.
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { SETTINGS_SECTION_PLACEHOLDER_TITLE, SettingsSectionPlaceholder } from "./SettingsSectionPlaceholder.js";

describe("SettingsSectionPlaceholder", () => {
  it("states that the section is not connected yet", () => {
    render(<SettingsSectionPlaceholder />);
    expect(screen.getByRole("heading", { name: SETTINGS_SECTION_PLACEHOLDER_TITLE })).toBeInTheDocument();
  });

  it("carries no interactive or file affordances", () => {
    const { container } = render(<SettingsSectionPlaceholder />);
    expect(container.querySelectorAll("button, input, a, [draggable='true'], [contenteditable='true']")).toHaveLength(
      0
    );
  });
});
