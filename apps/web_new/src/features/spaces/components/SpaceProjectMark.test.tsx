// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { SpaceBoardProject } from "@pidash/api-client";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { resolveSpaceMarkKind, SpaceProjectMark, type SpaceMarkKind } from "./SpaceProjectMark.js";

function detailsWith(logoProps: Record<string, unknown>): SpaceBoardProject {
  return {
    id: "11111111-1111-4111-8111-111111111111",
    identifier: "SPC",
    name: "Public board",
    cover_image: null,
    cover_image_url: null,
    logo_props: logoProps,
    description: "",
    is_default: false,
  };
}

function markKind(): string | null {
  return screen.getByTestId("space-navbar-mark").getAttribute("data-mark");
}

describe("resolveSpaceMarkKind", () => {
  const cases: Array<[string, SpaceBoardProject | null, SpaceMarkKind | "throw"]> = [
    ["icon branch", detailsWith({ in_use: "icon", icon: { name: "rocket_launch", color: "#1075e1" } }), "icon"],
    ["unknown icon name keeps the icon branch", detailsWith({ in_use: "icon", icon: { name: "rocket" } }), "icon"],
    ["emoji branch", detailsWith({ in_use: "emoji", emoji: { value: "128512" } }), "emoji"],
    ["empty settings stay empty", detailsWith({}), "empty"],
    ["uploads are never rendered", detailsWith({ in_use: "upload", url: "https://cdn.test/mark.png" }), "empty"],
    ["missing details fall back", null, "fallback"],
    ["non-map logo_props falls back", detailsWith("not-a-map" as unknown as Record<string, unknown>), "fallback"],
    ["null logo_props falls back", detailsWith(null as unknown as Record<string, unknown>), "fallback"],
    ["array logo_props falls back", detailsWith([] as unknown as Record<string, unknown>), "fallback"],
    [
      "out-of-range emoji throws to the boundary",
      detailsWith({ in_use: "emoji", emoji: { value: "99999999" } }),
      "throw",
    ],
    [
      "undecodable emoji throws to the boundary",
      detailsWith({ in_use: "emoji", emoji: { value: "not-a-code-point" } }),
      "throw",
    ],
  ];
  it.each(cases)("%s", (_label, details, kind) => {
    if (kind === "throw") {
      expect(() => resolveSpaceMarkKind(details)).toThrow(RangeError);
    } else {
      expect(resolveSpaceMarkKind(details)).toBe(kind);
    }
  });
});

describe("SpaceProjectMark", () => {
  it("renders the icon glyph in its stored color once the chunk lands", async () => {
    render(
      <SpaceProjectMark details={detailsWith({ in_use: "icon", icon: { name: "rocket_launch", color: "#1075e1" } })} />
    );
    expect(markKind()).toBe("icon");
    const svg = await screen.findByTestId("space-navbar-mark-glyph");
    expect(svg.tagName.toLowerCase()).toBe("svg");
    expect((svg as unknown as SVGElement).style.color).toBe("rgb(16, 117, 225)");
  });

  it("renders an uncolored glyph without a stored color", async () => {
    render(<SpaceProjectMark details={detailsWith({ in_use: "icon", icon: { name: "star" } })} />);
    expect(markKind()).toBe("icon");
    expect(await screen.findByTestId("space-navbar-mark-glyph")).toBeInTheDocument();
  });

  it("keeps the icon branch with the identifier initial for an unknown icon name", () => {
    const { container } = render(
      <SpaceProjectMark details={detailsWith({ in_use: "icon", icon: { name: "rocket" } })} />
    );
    expect(markKind()).toBe("icon");
    expect(screen.getByTestId("space-navbar-mark")).toHaveTextContent("S");
    expect(container.querySelector("svg")).toBeNull();
  });

  it("renders the decoded emoji glyph", () => {
    render(<SpaceProjectMark details={detailsWith({ in_use: "emoji", emoji: { value: "128512" } })} />);
    expect(markKind()).toBe("emoji");
    expect(screen.getByTestId("space-navbar-mark")).toHaveTextContent("😀");
  });

  it("renders an empty slot without inventing a mark", () => {
    render(<SpaceProjectMark details={detailsWith({})} />);
    expect(markKind()).toBe("empty");
    expect(screen.getByTestId("space-navbar-mark")).toHaveTextContent("");
  });

  it("renders a generic mark without project details", () => {
    const { container } = render(<SpaceProjectMark details={null} />);
    expect(markKind()).toBe("fallback");
    expect(container.querySelector("svg")).not.toBeNull();
    expect(screen.getByTestId("space-navbar-mark")).toHaveTextContent("");
  });

  it("renders the generic mark when logo_props is not a map", () => {
    render(<SpaceProjectMark details={detailsWith("not-a-map" as unknown as Record<string, unknown>)} />);
    expect(markKind()).toBe("fallback");
  });

  it("throws a corrupt emoji code point to the space error boundary (SPC-019)", () => {
    expect(() =>
      render(<SpaceProjectMark details={detailsWith({ in_use: "emoji", emoji: { value: "99999999" } })} />)
    ).toThrow(RangeError);
    expect(() =>
      render(<SpaceProjectMark details={detailsWith({ in_use: "emoji", emoji: { value: "not-a-code-point" } })} />)
    ).toThrow(RangeError);
  });
});
