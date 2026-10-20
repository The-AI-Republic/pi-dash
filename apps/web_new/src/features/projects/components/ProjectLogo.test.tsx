// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";
import { ProjectLogo, selectLogoGlyph, selectLogoImage } from "./ProjectLogo.js";

describe("selectLogoGlyph", () => {
  it("reads the picked emoji glyph", () => {
    expect(selectLogoGlyph({ in_use: "emoji", emoji: { value: "🚀" } })).toBe("🚀");
  });

  it("decodes a stored decimal code-point value", () => {
    expect(selectLogoGlyph({ in_use: "emoji", emoji: { value: "128640" } })).toBe("🚀");
  });

  it("reports an uploaded emblem as an image, not a glyph", () => {
    expect(selectLogoImage({ in_use: "upload", url: "/api/assets/v2/static/emblem/" })).toBe(
      "/api/assets/v2/static/emblem/"
    );
    expect(selectLogoImage({ in_use: "emoji", emoji: { value: "🚀" } })).toBeNull();
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

  it("renders an uploaded emblem as an image resolved against the API origin", () => {
    render(
      <ProjectLogo
        logoProps={{ in_use: "upload", url: "/api/assets/v2/static/emblem/" }}
        identifier="WEB"
        apiBaseUrl="https://api.test"
      />
    );
    expect(screen.getByTestId("project-logo")).toHaveAttribute("data-mark", "image");
    expect(screen.getByTestId("project-logo-image")).toHaveAttribute(
      "src",
      "https://api.test/api/assets/v2/static/emblem/"
    );
  });

  it("renders a decimal emoji value after a save", () => {
    render(<ProjectLogo logoProps={{ in_use: "emoji", emoji: { value: "128293" } }} identifier="WEB" />);
    expect(screen.getByTestId("project-logo")).toHaveTextContent("🔥");
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
