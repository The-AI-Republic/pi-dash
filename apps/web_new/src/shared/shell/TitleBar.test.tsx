// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// TitleBar desktop chrome (NEWFRONT-18): the drag region, the macOS
// traffic-light inset and the native title sync all key off
// platform.window, so the web build renders plain chrome.
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { platform, type TitleBarApi } from "../../core/platform/index.js";
import { platform as desktopPlatform } from "../../core/platform/tauri.js";
import { TitleBar } from "./TitleBar.js";

function renderBar(section = "Issues"): void {
  render(
    <TitleBar workspaceName="Acme" section={section} userLabel="ada@example.com" onOpenPalette={() => undefined} />
  );
}

describe("TitleBar platform chrome", () => {
  const hadWindow = platform.window;

  afterEach(() => {
    if (hadWindow === undefined) delete platform.window;
    else platform.window = hadWindow;
  });

  it("renders plain chrome on web (no window capability)", () => {
    delete platform.window;
    renderBar();
    const header = screen.getByRole("banner");
    expect(header).not.toHaveAttribute("data-tauri-drag-region");
    expect(header.className).not.toContain("pl-20");
    expect(screen.getByText("Pi Dash")).toBeInTheDocument();
  });

  it("adds the drag region, traffic-light inset and title sync on desktop", () => {
    const setTitle = vi.fn();
    const fake: TitleBarApi = { trafficLightInset: true, setTitle };
    platform.window = fake;
    renderBar("Issues");
    const header = screen.getByRole("banner");
    expect(header).toHaveAttribute("data-tauri-drag-region", "true");
    expect(header.className).toContain("pl-20");
    expect(setTitle).toHaveBeenCalledWith("Issues — Pi Dash");
  });

  it("keeps the drag region but skips the inset off macOS", () => {
    const setTitle = vi.fn();
    platform.window = { trafficLightInset: false, setTitle };
    renderBar("");
    const header = screen.getByRole("banner");
    expect(header).toHaveAttribute("data-tauri-drag-region");
    expect(header.className).not.toContain("pl-20");
    expect(setTitle).toHaveBeenCalledWith("Pi Dash");
  });
});

describe("desktop window capability", () => {
  it("exposes a boolean traffic-light inset", () => {
    expect(typeof desktopPlatform.window?.trafficLightInset).toBe("boolean");
  });

  it("setTitle updates the document title without a shell", () => {
    desktopPlatform.window?.setTitle("Issues — Pi Dash");
    expect(globalThis.document.title).toBe("Issues — Pi Dash");
  });
});
