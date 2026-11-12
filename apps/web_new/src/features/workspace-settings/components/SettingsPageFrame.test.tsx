// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Content frame (WSET-001): one section heading, an optional description
// slot, and the body beneath it. The heading and the body share a centred
// max-width column unless the page opts out with fullWidth.
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

  it("centres the body in a max-width column with the padding outside it", () => {
    render(
      <SettingsPageFrame title="General">
        <p>details</p>
      </SettingsPageFrame>
    );
    const column = screen.getByTestId("settings-page-content");
    expect(column).toHaveTextContent("details");
    expect(column).toHaveClass("mx-auto", "w-full", "max-w-(--settings-content-max)");
    expect(column).not.toHaveClass("px-(--space-6)", "py-(--space-6)");
    // The gutters stay on the full-width body so narrow windows keep them.
    expect(column.parentElement).toHaveClass("px-(--space-6)", "py-(--space-6)");
  });

  it("lines the header content up with the body column under a full-width border", () => {
    render(
      <SettingsPageFrame title="General">
        <p>details</p>
      </SettingsPageFrame>
    );
    const header = screen.getByTestId("settings-page-header-content");
    expect(header).toHaveClass("mx-auto", "w-full", "max-w-(--settings-content-max)");
    expect(header).toContainElement(screen.getByRole("heading", { level: 1, name: "General" }));
    const bar = header.parentElement;
    expect(bar?.tagName).toBe("HEADER");
    expect(bar).toHaveClass("border-b");
    expect(bar).not.toHaveClass("mx-auto", "max-w-(--settings-content-max)");
  });

  it("keeps the scroll area on the full-width frame", () => {
    const { container } = render(
      <SettingsPageFrame title="General">
        <p>details</p>
      </SettingsPageFrame>
    );
    const frame = container.firstElementChild;
    expect(frame).toHaveClass("overflow-y-auto");
    expect(frame).not.toHaveClass("mx-auto", "max-w-(--settings-content-max)");
  });

  it("renders full width when the page opts out of the column", () => {
    render(
      <SettingsPageFrame title="Members" fullWidth>
        <p>roster</p>
      </SettingsPageFrame>
    );
    expect(screen.queryByTestId("settings-page-content")).not.toBeInTheDocument();
    expect(screen.queryByTestId("settings-page-header-content")).not.toBeInTheDocument();
    expect(screen.getByText("roster")).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 1, name: "Members" })).toBeInTheDocument();
  });
});
