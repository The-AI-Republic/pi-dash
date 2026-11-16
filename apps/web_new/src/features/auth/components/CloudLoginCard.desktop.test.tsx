// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const platformHarness = vi.hoisted(() => ({ shell: undefined as unknown }));

vi.mock("../../../core/platform/index.js", () => ({
  get platform() {
    return { kind: "desktop", shellSignIn: platformHarness.shell };
  },
  toApiTransport: () => {
    throw new Error("unexpected transport resolution");
  },
}));

const location = vi.hoisted(() => ({
  origin: "tauri://localhost",
  search: "",
  assign: vi.fn(),
  replace: vi.fn(),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { CloudLoginCard } from "./CloudLoginCard.js";

function memoryStorage(initial: Record<string, string> = {}) {
  const entries = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => entries.get(key) ?? null,
    setItem: (key: string, value: string) => void entries.set(key, value),
    removeItem: (key: string) => void entries.delete(key),
    entries,
  };
}

beforeEach(() => {
  location.search = "";
  location.assign.mockClear();
  location.replace.mockClear();
  platformHarness.shell = undefined;
  vi.stubGlobal("sessionStorage", memoryStorage());
  Object.defineProperty(globalThis.window, "location", {
    value: location,
    writable: true,
    configurable: true,
  });
  createAppClient("https://api.test", async (url) => ({
    status: 200,
    url,
    headers: { get: () => null },
    text: async () => "{}",
  }));
});

afterEach(() => {
  resetAppClient();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("CloudLoginCard in the desktop shell", () => {
  it("renders the shared card instead of resuming silently", () => {
    location.search = "?next_path=%2Facme";
    platformHarness.shell = { bakedOrigin: "https://api.test", openSystemBrowser: vi.fn() };
    render(<CloudLoginCard />);
    expect(screen.getByRole("heading", { name: "Sign in to your workspace" })).toBeVisible();
    expect(screen.getByRole("button", { name: /sign in with ai republic/i })).toBeVisible();
    expect(location.replace).not.toHaveBeenCalled();
  });

  it("opens the marked handoff in the system browser, never the webview", async () => {
    const user = userEvent.setup();
    location.search = "?next_path=%2Facme";
    const openSystemBrowser = vi.fn(async (_url: string) => undefined);
    platformHarness.shell = { bakedOrigin: "https://api.test/", openSystemBrowser };
    render(<CloudLoginCard />);
    await user.click(screen.getByRole("button", { name: /sign in with ai republic/i }));
    expect(location.assign).not.toHaveBeenCalled();
    expect(openSystemBrowser).toHaveBeenCalledTimes(1);
    const url = new URL(String(openSystemBrowser.mock.calls[0]?.[0]));
    expect(`${url.origin}${url.pathname}`).toBe("https://api.test/api/auth/sign-in/");
    // The webview origin is opaque, so no carried target can validate and
    // the handoff fails closed to the app root; the deep link returns
    // through the shell, never through a browser origin.
    expect(url.searchParams.get("return_to")).toBe("/");
    expect(url.searchParams.get("desktop")).toBe("1");
    expect(url.searchParams.has("return_origin")).toBe(false);
  });

  it("refuses loudly without a baked origin and stays on the card", async () => {
    const user = userEvent.setup();
    platformHarness.shell = { bakedOrigin: undefined, openSystemBrowser: vi.fn() };
    const logged: string[] = [];
    vi.spyOn(console, "error").mockImplementation((message?: unknown) => {
      logged.push(String(message));
    });
    render(<CloudLoginCard />);
    await user.click(screen.getByRole("button", { name: /sign in with ai republic/i }));
    expect(location.assign).not.toHaveBeenCalled();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(logged.some((line) => line.includes("VITE_API_BASE_URL"))).toBe(true);
  });

  it("surfaces a blocked browser open as an alert instead of hanging", async () => {
    const user = userEvent.setup();
    platformHarness.shell = {
      bakedOrigin: "https://api.test",
      openSystemBrowser: async () => {
        throw new Error("opener blocked");
      },
    };
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    render(<CloudLoginCard />);
    await user.click(screen.getByRole("button", { name: /sign in with ai republic/i }));
    expect(location.assign).not.toHaveBeenCalled();
    expect(await screen.findByRole("alert")).toHaveTextContent("system browser could not be opened");
  });
});
