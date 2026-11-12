// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { WorkspaceLogo } from "./WorkspaceLogo.js";

describe("WorkspaceLogo", () => {
  it("renders the uploaded image under the logo label", () => {
    render(<WorkspaceLogo src="https://cdn.example/logo.png" name="Acme" />);
    const mark = screen.getByTestId("workspace-logo");
    expect(mark).toHaveAttribute("aria-label", "Workspace logo");
    const image = mark.querySelector("img");
    expect(image?.getAttribute("src")).toBe("https://cdn.example/logo.png");
  });

  it("renders the raw initial under the workspace label without a logo", () => {
    render(<WorkspaceLogo src={null} name="acme works" />);
    const mark = screen.getByTestId("workspace-logo");
    expect(mark).toHaveAttribute("aria-label", "acme works workspace");
    expect(mark.querySelector("img")).toBeNull();
    expect(mark.textContent).toBe("a");
  });

  it("falls back to the initial when the image breaks", () => {
    render(<WorkspaceLogo src="https://cdn.example/missing.png" name="Acme" />);
    const mark = screen.getByTestId("workspace-logo");
    fireEvent.error(mark.querySelector("img")!);
    expect(mark).toHaveAttribute("aria-label", "Acme workspace");
    expect(mark.querySelector("img")).toBeNull();
    expect(mark.textContent).toBe("A");
  });
});
