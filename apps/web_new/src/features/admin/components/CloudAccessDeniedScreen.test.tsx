// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { CLOUD_ACCESS_DENIED_TITLE, CloudAccessDeniedScreen } from "./CloudAccessDeniedScreen.js";

describe("CloudAccessDeniedScreen", () => {
  it("explains the verdict with the pinned heading", () => {
    render(<CloudAccessDeniedScreen />);
    expect(screen.getByTestId("admin-cloud-denied")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Access denied" })).toBeInTheDocument();
    expect(document.title).toBe(CLOUD_ACCESS_DENIED_TITLE);
  });

  it("offers only a plain full-navigation link back to the site root", () => {
    render(<CloudAccessDeniedScreen />);
    const links = screen.getAllByRole("link");
    expect(links).toHaveLength(1);
    expect(links[0]?.tagName).toBe("A");
    expect(links[0]).toHaveAttribute("href", "/");
    expect(links[0]).toHaveTextContent("Return to Pi Dash");
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("carries no password or setup fields", () => {
    render(<CloudAccessDeniedScreen />);
    expect(document.querySelector('input[type="password"]')).toBeNull();
    expect(screen.queryByLabelText("First name")).not.toBeInTheDocument();
  });
});
