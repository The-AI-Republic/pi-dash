// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// TitleBar desktop chrome (NEWFRONT-18): the drag region, the macOS
// traffic-light inset and the native title sync all key off
// platform.window, so the web build renders plain chrome.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient } from "../../core/api/index.js";
import { platform, type TitleBarApi } from "../../core/platform/index.js";
import { platform as desktopPlatform } from "../../core/platform/tauri.js";
import { TitleBar } from "./TitleBar.js";

function renderBar(section = "Issues"): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <TitleBar
        workspaceName="Acme"
        section={section}
        userLabel="ada@example.com"
        searchScope={null}
        onSelectSearchResult={() => undefined}
      />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  createAppClient("https://api.test", async (url) => ({
    status: 200,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify({ results: {} }),
  }));
});

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

describe("TitleBar search box", () => {
  it("mounts the always-visible palette search field (SHELL-081)", () => {
    const hadWindow = platform.window;
    delete platform.window;
    try {
      renderBar();
      expect(screen.getByPlaceholderText("Search commands...")).toBeInTheDocument();
    } finally {
      if (hadWindow !== undefined) platform.window = hadWindow;
    }
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
