// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { ProjectViewStar } from "./ProjectViewStar.js";

describe("ProjectViewStar", () => {
  it("offers starring an unstarred view", () => {
    const onToggle = vi.fn();
    render(<ProjectViewStar viewName="Triage" favorited={false} pending={false} onToggle={onToggle} />);

    const star = screen.getByRole("button", { name: "Add Triage to favorites" });
    expect(star).toHaveAttribute("aria-pressed", "false");
    expect(star.querySelector("svg")).toHaveAttribute("fill", "none");
    fireEvent.click(star);
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("offers unstarring a starred view", () => {
    render(<ProjectViewStar viewName="Triage" favorited={true} pending={false} onToggle={() => {}} />);

    const star = screen.getByRole("button", { name: "Remove Triage from favorites" });
    expect(star).toHaveAttribute("aria-pressed", "true");
    expect(star.querySelector("svg")).toHaveAttribute("fill", "currentColor");
    expect(star).toBeEnabled();
  });

  it("pins the star while its toggle flies", () => {
    const onToggle = vi.fn();
    render(<ProjectViewStar viewName="Triage" favorited={false} pending={true} onToggle={onToggle} />);

    const star = screen.getByRole("button", { name: "Add Triage to favorites" });
    expect(star).toBeDisabled();
    fireEvent.click(star);
    expect(onToggle).not.toHaveBeenCalled();
  });
});
