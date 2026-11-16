// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { RepoStar, REPOSITORY_URL, starIconSrc } from "./RepoStar.js";

describe("starIconSrc", () => {
  it("takes white on dark bases and black everywhere else", () => {
    expect(starIconSrc("dark")).toMatch(/github-white\.png$/);
    expect(starIconSrc("dark-contrast")).toMatch(/github-white\.png$/);
    expect(starIconSrc("custom", true)).toMatch(/github-white\.png$/);
    expect(starIconSrc("light")).toMatch(/github-black\.png$/);
    expect(starIconSrc("light-contrast")).toMatch(/github-black\.png$/);
    expect(starIconSrc("custom", false)).toMatch(/github-black\.png$/);
    expect(starIconSrc("custom")).toMatch(/github-black\.png$/);
  });
});

describe("RepoStar", () => {
  afterEach(() => {
    document.documentElement.removeAttribute("data-theme");
  });

  it("opens the repository externally with safe attributes", () => {
    render(<RepoStar />);
    const link = screen.getByRole("link", { name: "Star us on GitHub" });
    expect(link).toHaveAttribute("href", REPOSITORY_URL);
    expect(REPOSITORY_URL).toContain("github.com/The-AI-Republic/pi-dash");
    expect(link).toHaveAttribute("target", "_blank");
    expect(link.getAttribute("rel")).toContain("noopener");
    expect(link.getAttribute("rel")).toContain("noreferrer");
  });

  it("swaps the icon with the document scheme without a provider", () => {
    document.documentElement.setAttribute("data-theme", "dark");
    const { unmount } = render(<RepoStar />);
    expect(screen.getByRole("img", { name: "GitHub" })).toHaveAttribute("src", "/logos/github-white.png");
    unmount();
    document.documentElement.setAttribute("data-theme", "light");
    render(<RepoStar />);
    expect(screen.getByRole("img", { name: "GitHub" })).toHaveAttribute("src", "/logos/github-black.png");
  });

  it("falls back to the black icon without any mark", () => {
    render(<RepoStar />);
    expect(screen.getByRole("img", { name: "GitHub" })).toHaveAttribute("src", "/logos/github-black.png");
  });
});
