// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  kind: "web" as "web" | "desktop",
  openExternal: vi.fn<(url: string) => Promise<void>>(async () => undefined),
}));

vi.mock("../../core/platform/index.js", () => ({
  platform: {
    get kind() {
      return harness.kind;
    },
    openExternal: (url: string) => harness.openExternal(url),
  },
}));

import { HOSTED_SCREEN_FAILURE_HEADING, HOSTED_SCREEN_HEADING } from "../coexistence/DesktopHostedScreen.js";
import { NOT_FOUND_ART, NOT_FOUND_ROBOTS, NOT_FOUND_TITLE, NotFoundPage } from "./NotFoundPage.js";

beforeEach(() => {
  harness.kind = "web";
  harness.openExternal.mockReset();
  harness.openExternal.mockImplementation(async () => undefined);
  window.history.replaceState(null, "", "/acme/__no_such_page__");
});

afterEach(() => {
  vi.unstubAllGlobals();
});

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

  it("keeps the not-found page on the web for an address the hosted app owns", () => {
    // The web build is the hosted app: an old-app screen lives behind the
    // same origin, so a desktop-only hand-off must not take over the web 404.
    window.history.replaceState(null, "", "/acme/projects/archives");
    render(<NotFoundPage />);
    expect(screen.getByRole("heading", { name: "That page does not exist" })).toBeInTheDocument();
    expect(harness.openExternal).not.toHaveBeenCalled();
  });

  it("keeps the not-found page on the desktop for a migrated address that resolves to nothing", async () => {
    // An unknown workspace slug matches the workspace route and then fails
    // its gate: a real in-window not-found, not a screen for the browser.
    harness.kind = "desktop";
    window.history.replaceState(null, "", "/no-such-workspace");
    render(<NotFoundPage />);
    expect(await screen.findByRole("heading", { name: "That page does not exist" })).toBeInTheDocument();
    expect(harness.openExternal).not.toHaveBeenCalled();
  });

  it("hands an unmigrated address to the browser on the desktop", async () => {
    // The bundle cannot tell an old-app screen from a typo, so an address
    // it does not own leaves the window; the hosted app answers it either
    // way (the page, or its own not-found at the same address).
    harness.kind = "desktop";
    vi.stubGlobal("__PIDASH_WEB_BASE_URL__", "https://app.example.test");
    window.history.replaceState(null, "", "/acme/__no_such_page__");
    render(<NotFoundPage />);
    await waitFor(() =>
      expect(harness.openExternal).toHaveBeenCalledWith("https://app.example.test/acme/__no_such_page__")
    );
  });

  it("keeps the root in the window on the desktop", async () => {
    harness.kind = "desktop";
    window.history.replaceState(null, "", "/");
    render(<NotFoundPage />);
    expect(await screen.findByRole("heading", { name: "That page does not exist" })).toBeInTheDocument();
  });

  it("holds a bare backdrop while the desktop hand-off chunk loads", () => {
    // The split must not let the window blink the "does not exist" page
    // under an address that exists in the hosted app: the first frame is a
    // plain surface, and the card replaces it when the chunk arrives.
    harness.kind = "desktop";
    window.history.replaceState(null, "", "/acme/projects/archives");
    const { container } = render(<NotFoundPage />);
    expect(screen.queryByRole("heading", { name: "That page does not exist" })).toBeNull();
    expect(container.querySelector("main")).not.toBeNull();
  });

  it("hands a migrated-elsewhere address to the browser on the desktop", async () => {
    harness.kind = "desktop";
    vi.stubGlobal("__PIDASH_WEB_BASE_URL__", "https://app.example.test");
    window.history.replaceState(null, "", "/acme/projects/archives");
    render(<NotFoundPage />);

    await waitFor(() =>
      expect(harness.openExternal).toHaveBeenCalledWith("https://app.example.test/acme/projects/archives")
    );
    expect(await screen.findByRole("heading", { name: HOSTED_SCREEN_HEADING })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "That page does not exist" })).toBeNull();
  });

  it("explains itself on the desktop when no hosted address is baked", async () => {
    harness.kind = "desktop";
    window.history.replaceState(null, "", "/acme/projects/archives");
    render(<NotFoundPage />);

    expect(await screen.findByRole("heading", { name: HOSTED_SCREEN_FAILURE_HEADING })).toBeInTheDocument();
    expect(harness.openExternal).not.toHaveBeenCalled();
  });
});
