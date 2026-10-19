// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const location = vi.hoisted(() => ({
  origin: "https://app.test",
  search: "",
  assign: vi.fn(),
  replace: vi.fn(),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { EXPLICIT_LOGOUT_KEY } from "../cloudLogin.js";
import { CloudLoginCard } from "./CloudLoginCard.js";

/** The URL handed to a navigation stub on its first call. */
function navigatedUrl(spy: { mock: { calls: unknown[][] } }): URL {
  const call = spy.mock.calls[0];
  if (call === undefined) throw new Error("no navigation recorded");
  return new URL(call[0] as string);
}

function memoryStorage(initial: Record<string, string> = {}) {
  const entries = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => entries.get(key) ?? null,
    setItem: (key: string, value: string) => void entries.set(key, value),
    removeItem: (key: string) => void entries.delete(key),
    entries,
  };
}

let storage: ReturnType<typeof memoryStorage>;

beforeEach(() => {
  location.search = "";
  location.assign.mockClear();
  location.replace.mockClear();
  storage = memoryStorage();
  vi.stubGlobal("sessionStorage", storage);
  Object.defineProperty(globalThis.window, "location", {
    value: location,
    writable: true,
    configurable: true,
  });
  createAppClient("https://app.test", async (url) => ({
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

describe("CloudLoginCard", () => {
  it("offers one continue action, the account line, legal links and the home link", () => {
    render(<CloudLoginCard />);

    expect(screen.getByRole("heading", { name: "Sign in to Pi Dash" })).toBeVisible();
    expect(screen.getByText("Continue with your AI Republic account.")).toBeVisible();
    expect(screen.getByRole("link", { name: /continue with ai republic/i })).toHaveAttribute(
      "href",
      "/api/auth/sign-in/"
    );
    expect(screen.getByRole("button", { name: /continue with ai republic/i })).toBeVisible();
    expect(screen.getAllByRole("button")).toHaveLength(1);
    expect(screen.getByRole("link", { name: "Terms" })).toHaveAttribute("href", "https://airepublic.com/terms");
    expect(screen.getByRole("link", { name: "Privacy Policy" })).toHaveAttribute(
      "href",
      "https://airepublic.com/privacy"
    );
    expect(screen.getAllByRole("link", { name: "Explore Pi Dash" })[0]).toHaveAttribute("href", "/home");
    expect(screen.queryByRole("alert")).toBeNull();
    expect(location.replace).not.toHaveBeenCalled();
    expect(location.assign).not.toHaveBeenCalled();
  });

  it("hands a manual attempt to the provider at the app root", async () => {
    const user = userEvent.setup();
    render(<CloudLoginCard />);

    await user.click(screen.getByRole("button", { name: /continue with ai republic/i }));

    expect(location.assign).toHaveBeenCalledTimes(1);
    const url = navigatedUrl(location.assign);
    expect(url.pathname).toBe("/api/auth/sign-in/");
    expect(url.searchParams.get("return_to")).toBe("/");
    expect(url.searchParams.has("return_origin")).toBe(false);
  });

  it("resumes silently with a pending target using a replacing navigation", async () => {
    location.search = "?next_path=%2Fdocs";
    render(<CloudLoginCard />);

    await waitFor(() => expect(location.replace).toHaveBeenCalledTimes(1));
    const url = navigatedUrl(location.replace);
    expect(url.pathname).toBe("/api/auth/sign-in/");
    expect(url.searchParams.get("return_to")).toBe("/docs");
    expect(screen.queryByRole("heading", { name: "Sign in to Pi Dash" })).toBeNull();
    expect(location.assign).not.toHaveBeenCalled();
  });

  it("keeps the card with a resume-failure alert when an error landed", () => {
    location.search = "?next_path=%2Fdocs&error=oidc_down&sso=failed";
    render(<CloudLoginCard />);

    expect(screen.getByRole("button", { name: /continue with ai republic/i })).toBeVisible();
    expect(screen.getByRole("alert")).toHaveTextContent(/automatically/i);
    expect(location.replace).not.toHaveBeenCalled();
    expect(location.assign).not.toHaveBeenCalled();
  });

  it("keeps the card while the explicit-logout marker is set", () => {
    location.search = "?next_path=%2Fdocs";
    storage.entries.set(EXPLICIT_LOGOUT_KEY, "1");
    render(<CloudLoginCard />);

    expect(screen.getByRole("heading", { name: "Sign in to Pi Dash" })).toBeVisible();
    expect(location.replace).not.toHaveBeenCalled();
  });

  it("carries the pending target on a manual attempt and clears the marker", async () => {
    location.search = "?next_path=%2Fdocs";
    storage.entries.set(EXPLICIT_LOGOUT_KEY, "1");
    const user = userEvent.setup();
    render(<CloudLoginCard />);

    await user.click(screen.getByRole("button", { name: /continue with ai republic/i }));

    const url = navigatedUrl(location.assign);
    expect(url.searchParams.get("return_to")).toBe("/docs");
    expect(storage.entries.has(EXPLICIT_LOGOUT_KEY)).toBe(false);
  });

  it("discards unsafe targets and renders the card", () => {
    for (const unsafe of [
      "//evil.example/steal",
      "https://evil.example/steal",
      "http://localhost:13038/downgrade",
      "/\\evil.example/steal",
      "workspace/issues/1",
    ]) {
      location.search = `?next_path=${encodeURIComponent(unsafe)}`;
      const { unmount } = render(<CloudLoginCard />);
      expect(screen.getByRole("button", { name: /continue with ai republic/i })).toBeVisible();
      expect(location.replace).not.toHaveBeenCalled();
      unmount();
    }
  });

  it("keeps the card on a bare visit", () => {
    render(<CloudLoginCard />);
    expect(screen.getByRole("heading", { name: "Sign in to Pi Dash" })).toBeVisible();
    expect(location.replace).not.toHaveBeenCalled();
  });

  it("returns to the root instead of looping onto login", async () => {
    // A login target still counts as a pending target, so the visit resumes;
    // the handoff's target is the root rather than the login page itself.
    location.search = "?next_path=%2Flogin";
    render(<CloudLoginCard />);

    await waitFor(() => expect(location.replace).toHaveBeenCalledTimes(1));
    const url = navigatedUrl(location.replace);
    expect(url.searchParams.get("return_to")).toBe("/");
    expect(location.assign).not.toHaveBeenCalled();
  });

  it("drops the fragment from a pending target", async () => {
    location.search = `?next_path=${encodeURIComponent("/docs#token")}`;
    render(<CloudLoginCard />);

    await waitFor(() => expect(location.replace).toHaveBeenCalledTimes(1));
    const url = navigatedUrl(location.replace);
    expect(url.searchParams.get("return_to")).toBe("/docs");
  });

  it("survives missing session storage", () => {
    vi.stubGlobal("sessionStorage", undefined);
    location.search = "?next_path=%2Fdocs";
    render(<CloudLoginCard />);
    // Without a readable marker the visit still resumes.
    expect(location.replace).toHaveBeenCalledTimes(1);
  });
});
