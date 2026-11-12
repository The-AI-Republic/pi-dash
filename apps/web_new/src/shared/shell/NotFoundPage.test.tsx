// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { NOT_FOUND_ART, NOT_FOUND_ROBOTS, NOT_FOUND_TITLE, NotFoundPage } from "./NotFoundPage.js";

describe("NotFoundPage", () => {
  it("renders an illustrated surface with explanatory copy and a way home", () => {
    render(<NotFoundPage />);

    expect(screen.getByRole("heading", { name: "That page does not exist" })).not.toBeNull();
    const body = screen.getByText(/mistyped/, { exact: false });
    expect(body.textContent!.length).toBeGreaterThan(20);
    // The cloud flag-off scenario matches unknown routes against
    // /cannot be found|not found|something went wrong/ (CLOUD-021).
    expect(body.textContent).toMatch(/not found/);

    const home = screen.getByRole("link", { name: "Back to home" });
    expect(home).toHaveAttribute("href", "/");

    const art = screen.getByRole("img");
    expect(art).toHaveAttribute("src", NOT_FOUND_ART);
    expect(art.getAttribute("alt") ?? "").not.toHaveLength(0);
  });

  it("marks the tab 404 and keeps crawlers out", () => {
    render(<NotFoundPage />);
    expect(document.title).toContain("404");
    expect(document.title).toBe(NOT_FOUND_TITLE);
    expect(document.querySelector('meta[name="robots"]')?.getAttribute("content")).toBe(NOT_FOUND_ROBOTS);
    expect(NOT_FOUND_ROBOTS).toBe("noindex, nofollow");
  });
});
