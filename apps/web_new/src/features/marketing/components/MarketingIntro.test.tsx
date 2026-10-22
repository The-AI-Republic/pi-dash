// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Frame-mounted content pages (CLOUD-004 keeps the frame on every public
// page; the bodies are NEWFRONT-648's rows).
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";

import { REPOSITORY_URL } from "../content.js";
import { MarketingIntro } from "./MarketingIntro.js";

describe("MarketingIntro", () => {
  it.each(["/docs", "/downloads", "/pricing"] as const)("renders a single heading for %s", (path) => {
    const { container } = render(<MarketingIntro path={path} />);
    expect(container.querySelectorAll("h1")).toHaveLength(1);
    expect(screen.getByRole("heading", { level: 1 }).textContent?.length).toBeGreaterThan(0);
  });

  it("links sign-in and the public repository", () => {
    const { container } = render(<MarketingIntro path="/downloads" />);
    const hrefs = [...container.querySelectorAll("a")].map((anchor) => anchor.getAttribute("href"));
    expect(hrefs).toEqual(["/login", REPOSITORY_URL]);
    expect(container.querySelector(`a[href="${REPOSITORY_URL}"]`)?.getAttribute("target")).toBe("_blank");
  });
});
