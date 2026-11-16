// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One preferences row (parity/specs/profile.md §7, PROF-016/017): heading,
// description, control and optional panel below.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { PreferenceRow } from "./PreferenceRow.js";

describe("PreferenceRow", () => {
  it("renders the heading, description and control", () => {
    render(
      <PreferenceRow
        title="Language"
        description="Change the interface language."
        control={<button type="button">English</button>}
      />
    );
    expect(screen.getByRole("heading", { level: 2, name: "Language" })).toBeInTheDocument();
    expect(screen.getByText("Change the interface language.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "English" })).toBeInTheDocument();
  });

  it("renders the panel below the row when one is given", () => {
    render(
      <PreferenceRow title="Theme" description="Pick a palette." control={<span>control</span>}>
        <p>designer</p>
      </PreferenceRow>
    );
    expect(screen.getByText("designer")).toBeInTheDocument();
  });

  it("omits the panel when there is none", () => {
    const { container } = render(
      <PreferenceRow title="Theme" description="Pick a palette." control={<span>control</span>} />
    );
    expect(container.querySelectorAll("[data-testid=preference-row] p")).toHaveLength(1);
  });
});
