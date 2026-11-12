// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Marketing backdrop (CLOUD-004): the world map asset paints the page, not
// a hand-drawn shape and not inline markup.
import { render } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { MarketingBackdrop, WORLD_MAP_ASSET, WORLD_MAP_MASK } from "./Backdrop.js";

describe("MarketingBackdrop", () => {
  it("points the mask at the world-map asset served from public/", () => {
    expect(WORLD_MAP_ASSET).toBe("/world-map.svg");
    expect(WORLD_MAP_MASK.maskImage).toBe('url("/world-map.svg")');
    expect(WORLD_MAP_MASK.maskSize).toBe("contain");
    expect(WORLD_MAP_MASK.maskPosition).toBe("center");
    expect(WORLD_MAP_MASK.maskRepeat).toBe("no-repeat");
  });

  it("renders one aria-hidden, non-interactive layer painted by the map", () => {
    const { container } = render(<MarketingBackdrop />);
    const layers = container.querySelectorAll("[data-marketing-backdrop]");
    expect(layers).toHaveLength(1);
    const layer = layers[0] as HTMLElement;
    expect(layer.getAttribute("aria-hidden")).toBe("true");
    expect(layer.style.maskImage).toContain("world-map.svg");
    expect(layer.querySelector("svg, a, button, input, img")).toBeNull();
    const className = layer.getAttribute("class") ?? "";
    expect(className).toContain("pointer-events-none");
    expect(className).toContain("fixed");
    expect(className).toContain("opacity-[0.055]");
    expect(className).toContain("bg-(--text)");
  });

  it("covers the viewport behind the frame content", () => {
    const { container } = render(
      <div className="relative isolate">
        <MarketingBackdrop />
        <main className="relative z-10">content</main>
      </div>
    );
    const layer = container.querySelector("[data-marketing-backdrop]") as HTMLElement;
    const className = layer.getAttribute("class") ?? "";
    expect(className).toContain("inset-0");
    expect(className).toContain("z-0");
    expect(className).not.toContain("-z-");
  });
});
