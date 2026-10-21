// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { ProjectMark, resolveMarkSource } from "./ProjectMark.js";

describe("ProjectMark", () => {
  it("draws an emoji mark as its glyph", () => {
    render(<ProjectMark logoProps={{ in_use: "emoji", emoji: { value: "🌱" } }} identifier="SEED" />);
    const mark = screen.getByTestId("project-mark");
    expect(mark).toHaveAttribute("data-mark", "emoji");
    expect(mark).toHaveTextContent("🌱");
    expect(mark.querySelector("svg")).toBeNull();
  });

  it("draws a configured icon in its own color at the requested size", async () => {
    render(
      <ProjectMark
        logoProps={{ in_use: "icon", icon: { name: "rocket_launch", color: "#f97316" } }}
        identifier="SEED"
        size={24}
        className="custom"
      />
    );
    const mark = screen.getByTestId("project-mark");
    expect(mark).toHaveAttribute("data-mark", "icon");
    expect(mark).toHaveStyle({ width: "24px", height: "24px" });
    expect(mark).toHaveClass("custom");
    await waitFor(() => expect(mark.querySelector("svg")).not.toBeNull());
    const glyph = mark.querySelector("svg");
    expect(glyph!.getAttribute("stroke")).toBe("#f97316");
    expect(glyph!.getAttribute("width")).toBe("17");
  });

  it("leaves the icon in its inherited color when none is stored", async () => {
    render(<ProjectMark logoProps={{ in_use: "icon", icon: { name: "star" } }} identifier="SEED" size={16} />);
    const mark = screen.getByTestId("project-mark");
    expect(mark).toHaveAttribute("data-mark", "icon");
    await waitFor(() => expect(mark.querySelector("svg")).not.toBeNull());
    expect(mark.querySelector("svg")!.getAttribute("stroke")).toBe("currentColor");
  });

  it("falls back to the initial for an unknown icon or an empty logo", async () => {
    const { rerender } = render(
      <ProjectMark logoProps={{ in_use: "icon", icon: { name: "no_such_symbol" } }} identifier="SEED" />
    );
    const mark = screen.getByTestId("project-mark");
    expect(mark).toHaveAttribute("data-mark", "icon");
    await waitFor(() => expect(mark.querySelector("svg")).toBeNull());
    expect(mark).toHaveTextContent("S");
    rerender(<ProjectMark logoProps={{}} identifier="" />);
    expect(screen.getByTestId("project-mark")).toHaveAttribute("data-mark", "initial");
    expect(screen.getByTestId("project-mark")).toHaveTextContent("?");
  });

  it("stays decorative for assistive tech", () => {
    render(<ProjectMark logoProps={{}} identifier="SEED" />);
    expect(screen.getByTestId("project-mark")).toHaveAttribute("aria-hidden", "true");
  });

  it("draws an uploaded emblem as a decorative image against the API base", () => {
    render(
      <ProjectMark
        logoProps={{ in_use: "emoji", emoji: { url: "/api/assets/v2/static/emblem/" } }}
        identifier="SEED"
        apiBaseUrl="https://api.test"
      />
    );
    const mark = screen.getByTestId("project-mark");
    expect(mark).toHaveAttribute("data-mark", "image");
    const image = screen.getByTestId("project-mark-image");
    expect(image).toHaveAttribute("src", "https://api.test/api/assets/v2/static/emblem/");
    expect(image).toHaveAttribute("alt", "");
  });
});

describe("resolveMarkSource", () => {
  it("resolves a root-relative path against the trimmed base", () => {
    expect(resolveMarkSource("/asset.png", "https://api.test/")).toBe("https://api.test/asset.png");
    expect(resolveMarkSource("asset.png", "https://api.test")).toBe("https://api.test/asset.png");
  });

  it("keeps an absolute, protocol-relative or baseless URL as stored", () => {
    expect(resolveMarkSource("https://cdn.test/a.png", "https://api.test")).toBe("https://cdn.test/a.png");
    expect(resolveMarkSource("//cdn.test/a.png", "https://api.test")).toBe("//cdn.test/a.png");
    expect(resolveMarkSource("/asset.png", undefined)).toBe("/asset.png");
    expect(resolveMarkSource("/asset.png", "")).toBe("/asset.png");
  });
});
