// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { ProjectLogo, selectLogoGlyph } from "./ProjectLogo.js";

afterEach(() => {
  resetAppClient();
});

describe("selectLogoGlyph", () => {
  it("reads the picked emoji glyph", () => {
    expect(selectLogoGlyph({ in_use: "emoji", emoji: { value: "🚀" } })).toBe("🚀");
  });

  it("decodes a stored decimal code-point value", () => {
    expect(selectLogoGlyph({ in_use: "emoji", emoji: { value: "128640" } })).toBe("🚀");
  });

  it("reads an uploaded emblem as an image, not a glyph", () => {
    expect(selectLogoGlyph({ in_use: "upload", url: "/api/assets/v2/static/emblem/" })).toBeNull();
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
    createAppClient("https://api.test", async () => new Response("{}"));
    render(<ProjectLogo logoProps={{ in_use: "upload", url: "/api/assets/v2/static/emblem/" }} identifier="WEB" />);
    expect(screen.getByTestId("project-logo")).toHaveAttribute("data-mark", "image");
    expect(screen.getByTestId("project-logo-image")).toHaveAttribute(
      "src",
      "https://api.test/api/assets/v2/static/emblem/"
    );
  });

  it("keeps an absolute emblem URL as stored", () => {
    createAppClient("https://api.test", async () => new Response("{}"));
    render(<ProjectLogo logoProps={{ in_use: "upload", url: "https://cdn.test/a.png" }} identifier="WEB" />);
    expect(screen.getByTestId("project-logo-image")).toHaveAttribute("src", "https://cdn.test/a.png");
  });

  it("renders a chosen symbol in its colour", async () => {
    render(
      <ProjectLogo logoProps={{ in_use: "icon", icon: { name: "rocket_launch", color: "#f97316" } }} identifier="WEB" />
    );
    const logo = screen.getByTestId("project-logo");
    expect(logo).toHaveAttribute("data-mark", "icon");
    // The glyph set is its own chunk; the initial stands in until it lands.
    expect(logo).toHaveTextContent("W");
    const icon = await screen.findByTestId("project-logo-icon");
    expect(logo).not.toHaveTextContent("W");
    expect(icon.getAttribute("style")).toContain("color");
  });

  it("draws a symbol without a colour in the inherited tone", async () => {
    render(<ProjectLogo logoProps={{ in_use: "icon", icon: { name: "star" } }} identifier="WEB" />);
    const icon = await screen.findByTestId("project-logo-icon");
    expect(icon).not.toHaveAttribute("style");
    expect(icon).not.toHaveAttribute("color");
    expect(icon.getAttribute("stroke")).toBe("currentColor");
  });

  it("falls back to the initial for a symbol name this frontend cannot draw", async () => {
    render(
      <ProjectLogo logoProps={{ in_use: "icon", icon: { name: "not_a_symbol", color: "#f97316" } }} identifier="WEB" />
    );
    await waitFor(() => expect(screen.getByTestId("project-logo")).toHaveTextContent("W"));
    expect(screen.queryByTestId("project-logo-icon")).not.toBeInTheDocument();
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
