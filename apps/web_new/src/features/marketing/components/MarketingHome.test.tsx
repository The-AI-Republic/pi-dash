// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Marketing home (CLOUD-003). Section order, counts and destinations are
// what the oracle pins; copy is free.
import { render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { REPOSITORY_URL } from "../content.js";
import { MarketingHome } from "./MarketingHome.js";

describe("MarketingHome", () => {
  it("offers exactly the three hero actions in oracle order", () => {
    const { container } = render(<MarketingHome />);
    const heroLinks = [...container.querySelectorAll("section")[0]!.querySelectorAll("a")].map((anchor) =>
      anchor.getAttribute("href")
    );
    expect(heroLinks).toEqual(["/login", "/downloads", "/docs"]);
  });

  it("follows the hero with the four loop items, four steps, four capabilities, repo panel and closing action", () => {
    const { container } = render(<MarketingHome />);
    const sections = [...container.querySelectorAll("section")];
    expect(sections).toHaveLength(6);
    expect(sections[1]!.querySelectorAll("div > div > span")).toHaveLength(12);
    expect(sections[1]!.querySelectorAll(":scope > div > div")).toHaveLength(4);
    expect(sections[2]!.querySelectorAll("ol > li")).toHaveLength(4);
    expect(sections[3]!.querySelectorAll("article")).toHaveLength(4);
    const repo = sections[4]!.querySelector(`a[href="${REPOSITORY_URL}"]`);
    expect(repo?.getAttribute("target")).toBe("_blank");
    expect(sections[5]!.querySelector("a")?.getAttribute("href")).toBe("/login");
  });

  it("owns the page's only top-level heading", () => {
    const { container } = render(<MarketingHome />);
    expect(container.querySelectorAll("h1")).toHaveLength(1);
    expect(container.querySelector("h1")?.textContent?.length).toBeGreaterThan(0);
  });

  it("illustrates a representative agent run without data or behavior", () => {
    render(<MarketingHome />);
    const illustration = screen.getByRole("img");
    expect(illustration.getAttribute("aria-label")).toMatch(/agent run/i);
    expect(within(illustration).queryByRole("link")).toBeNull();
    expect(within(illustration).queryByRole("button")).toBeNull();
  });
});
