// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { LAYOUT_ORDER, type LayoutKind } from "../layouts/kinds.js";
import { LayoutIcon } from "./LayoutIcon.js";

describe("LayoutIcon", () => {
  it.each(LAYOUT_ORDER.map((kind) => [kind]))("renders a hidden glyph for %s", (kind) => {
    const { container } = render(<LayoutIcon kind={kind as LayoutKind} />);
    const svg = container.querySelector("svg");
    expect(svg).not.toBeNull();
    expect(svg).toHaveAttribute("aria-hidden", "true");
  });

  it("honors an explicit size", () => {
    const { container } = render(<LayoutIcon kind="list" size={24} />);
    const svg = container.querySelector("svg");
    expect(svg).toHaveAttribute("width", "24");
    expect(svg).toHaveAttribute("height", "24");
  });
});
