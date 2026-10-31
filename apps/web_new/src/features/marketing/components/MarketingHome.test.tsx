// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Marketing home (CLOUD-003). The oracle reads the hero's action
// destinations, the four-item strip, the four steps, the four capability
// articles and the closing action; the layout assertions here keep the old
// page's two-column hero and its illustration panel.
import { render, screen, within } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { DELIVERY_STEPS, HERO_BADGE, HERO_HEADLINES, HERO_NOTE, REPOSITORY_URL, RUN_PREVIEW } from "../content.js";
import { MarketingHome } from "./MarketingHome.js";

function sections(container: HTMLElement): HTMLElement[] {
  return [...container.querySelectorAll("section")];
}

describe("MarketingHome", () => {
  it("offers exactly the three hero actions in oracle order", () => {
    const { container } = render(<MarketingHome />);
    const heroLinks = [...sections(container)[0]!.querySelectorAll("a")].map((anchor) => anchor.getAttribute("href"));
    expect(heroLinks).toEqual(["/login", "/downloads", "/docs"]);
  });

  it("lays the hero out as copy beside the run illustration", () => {
    const { container } = render(<MarketingHome />);
    const hero = sections(container)[0]!;
    expect(hero.className).toContain("lg:grid-cols-[1.02fr_0.98fr]");
    const illustration = within(hero).getByRole("img");
    expect(hero.contains(illustration)).toBe(true);
    expect(hero.lastElementChild).toBe(illustration);
  });

  it("carries the badge, a two-line headline, the positioning copy and the free-to-start note", () => {
    const { container } = render(<MarketingHome />);
    const hero = sections(container)[0]!;
    expect(hero.textContent).toContain(HERO_BADGE);
    const heading = within(hero).getByRole("heading", { level: 1 });
    expect([...heading.querySelectorAll("span")].map((line) => line.textContent)).toEqual([...HERO_HEADLINES]);
    expect(heading.className).toContain("clamp(3rem,7vw,5.8rem)");
    expect(hero.textContent).toContain(HERO_NOTE);
  });

  it("follows the hero with the value strip, four steps, four capabilities, repo panel and closing action", () => {
    const { container } = render(<MarketingHome />);
    const all = sections(container);
    expect(all).toHaveLength(6);
    expect(all[1]!.querySelectorAll(":scope > div > div")).toHaveLength(4);
    expect(all[2]!.querySelectorAll("ol > li")).toHaveLength(4);
    expect(all[2]!.querySelectorAll("ol > li")).toHaveLength(DELIVERY_STEPS.length);
    expect(all[2]!.textContent).toContain("01");
    expect(all[2]!.textContent).toContain("04");
    expect(all[3]!.querySelectorAll("article")).toHaveLength(4);
    const repo = all[4]!.querySelector(`a[href="${REPOSITORY_URL}"]`);
    expect(repo?.getAttribute("target")).toBe("_blank");
    expect(repo?.getAttribute("rel")).toBe("noopener noreferrer");
    expect(all[5]!.querySelector("a")?.getAttribute("href")).toBe("/login");
  });

  it("paints the closing action with the filled accent style", () => {
    const { container } = render(<MarketingHome />);
    const closing = sections(container)[5]!;
    const cta = closing.querySelector("a")!;
    expect(cta.className).toContain("bg-(--accent)");
    expect(cta.className).toContain("text-(--on-accent)");
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
    for (const step of RUN_PREVIEW.steps) expect(illustration.textContent).toContain(step.label);
    expect(illustration.textContent).toContain(RUN_PREVIEW.issue);
  });
});
