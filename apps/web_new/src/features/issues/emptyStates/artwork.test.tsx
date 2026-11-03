// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The inline empty-state art (parity/specs/issues.md §8): every scene is
// a decorative, text-free drawing so a screen reader announces only the
// card's heading.
import { render } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { ArchiveArt, FirstItemArt, NoMatchArt, ProfileArt, ProjectArt, ViewArt } from "./artwork.js";

const SCENES = [
  ["First work item", FirstItemArt],
  ["No matching results", NoMatchArt],
  ["Archived work items", ArchiveArt],
  ["Profile work items", ProfileArt],
  ["No projects", ProjectArt],
  ["Saved view", ViewArt],
] as const;

describe("empty-state artwork", () => {
  it.each(SCENES)("draws the %s scene as hidden decoration", (name, Scene) => {
    const { container } = render(<Scene />);
    const svg = container.querySelector("svg");
    expect(svg).not.toBeNull();
    expect(svg).toHaveAttribute("aria-hidden", "true");
    expect(svg).toHaveAttribute("data-artwork", name);
    expect(svg).toHaveAttribute("focusable", "false");
    expect(svg?.textContent).toBe("");
  });
});
