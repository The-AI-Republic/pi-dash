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

import { HOSTED_SCREEN_HEADING } from "./DesktopHostedScreen.js";
import { DesktopHandOffBoundary } from "./DesktopHandOffBoundary.js";

beforeEach(() => {
  harness.kind = "desktop";
  harness.openExternal.mockReset();
  harness.openExternal.mockImplementation(async () => undefined);
  window.history.replaceState(null, "", "/acme/__no_such_page__");
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("DesktopHandOffBoundary", () => {
  it("hands an address another frontend owns to the browser", async () => {
    vi.stubGlobal("__PIDASH_WEB_BASE_URL__", "https://app.example.test");
    window.history.replaceState(null, "", "/god-mode/users?tab=all#top");
    render(<DesktopHandOffBoundary />);

    await waitFor(() =>
      expect(harness.openExternal).toHaveBeenCalledWith("https://app.example.test/god-mode/users?tab=all#top")
    );
    expect(await screen.findByRole("heading", { name: HOSTED_SCREEN_HEADING })).toBeInTheDocument();
  });

  it("keeps a web-only subtree on the hand-off card, not the not-found page", async () => {
    vi.stubGlobal("__PIDASH_WEB_BASE_URL__", "https://app.example.test");
    window.history.replaceState(null, "", "/spaces/parity-anchor/");
    render(<DesktopHandOffBoundary />);

    expect(await screen.findByRole("heading", { name: HOSTED_SCREEN_HEADING })).toBeInTheDocument();
  });

  it("answers an address the bundle owns with the in-window not-found page", () => {
    window.history.replaceState(null, "", "/no-such-workspace");
    render(<DesktopHandOffBoundary />);

    expect(screen.getByRole("heading", { name: "That page does not exist" })).toBeInTheDocument();
    expect(harness.openExternal).not.toHaveBeenCalled();
  });

  it("never reaches for the browser outside the desktop build", () => {
    harness.kind = "web";
    window.history.replaceState(null, "", "/acme/projects/archives");
    render(<DesktopHandOffBoundary />);

    expect(screen.getByRole("heading", { name: "That page does not exist" })).toBeInTheDocument();
    expect(harness.openExternal).not.toHaveBeenCalled();
  });
});
