// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Content frame (WSET-001): one section heading, an optional description
// slot, and the body beneath it.
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { SettingsPageFrame } from "./SettingsPageFrame.js";

describe("SettingsPageFrame", () => {
  it("renders the section heading above its body", () => {
    render(
      <SettingsPageFrame title="Members">
        <p>roster</p>
      </SettingsPageFrame>
    );
    expect(screen.getByRole("heading", { level: 1, name: "Members" })).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Breadcrumb" })).toHaveTextContent("Members");
    expect(screen.getByText("roster")).toBeInTheDocument();
    expect(screen.queryByText("Who belongs here.")).not.toBeInTheDocument();
  });

  it("adds the description when one is given", () => {
    render(
      <SettingsPageFrame title="Members" description="Who belongs here.">
        <p>roster</p>
      </SettingsPageFrame>
    );
    expect(screen.getByText("Who belongs here.")).toBeInTheDocument();
  });
});
