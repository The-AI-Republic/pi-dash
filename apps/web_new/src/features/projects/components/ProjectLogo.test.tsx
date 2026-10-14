// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";
import { ProjectLogo, selectLogoGlyph } from "./ProjectLogo.js";

describe("selectLogoGlyph", () => {
  it("reads the picked emoji glyph", () => {
    expect(selectLogoGlyph({ in_use: "emoji", emoji: { value: "🚀" } })).toBe("🚀");
  });

  it("rejects icon picks and missing picks", () => {
    expect(selectLogoGlyph({ in_use: "icon", icon: { name: "anchor" } })).toBeNull();
    expect(selectLogoGlyph({})).toBeNull();
    expect(selectLogoGlyph({ in_use: null })).toBeNull();
  });

  it("rejects malformed emoji values", () => {
    expect(selectLogoGlyph({ in_use: "emoji" })).toBeNull();
    expect(selectLogoGlyph({ in_use: "emoji", emoji: "🚀" })).toBeNull();
    expect(selectLogoGlyph({ in_use: "emoji", emoji: {} })).toBeNull();
    expect(selectLogoGlyph({ in_use: "emoji", emoji: { value: 7 } })).toBeNull();
    expect(selectLogoGlyph({ in_use: "emoji", emoji: { value: "   " } })).toBeNull();
  });
});

describe("ProjectLogo", () => {
  it("renders the glyph when one is picked", () => {
    render(<ProjectLogo logoProps={{ in_use: "emoji", emoji: { value: "🚀" } }} identifier="WEB" />);
    expect(screen.getByTestId("project-logo")).toHaveTextContent("🚀");
  });

  it("falls back to the short-code initial", () => {
    render(<ProjectLogo logoProps={{}} identifier="web" />);
    expect(screen.getByTestId("project-logo")).toHaveTextContent("W");
  });

  it("falls back to a mark on an empty short code", () => {
    render(<ProjectLogo logoProps={{}} identifier="  " />);
    expect(screen.getByTestId("project-logo")).toHaveTextContent("?");
  });

  it("stays decorative", () => {
    render(<ProjectLogo logoProps={{}} identifier="WEB" />);
    expect(screen.getByTestId("project-logo")).toHaveAttribute("aria-hidden", "true");
  });
});
