// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const harness = vi.hoisted(() => ({
  openExternal: async (_url: string): Promise<void> => undefined,
}));

vi.mock("../../../core/platform/index.js", () => ({
  platform: {
    get openExternal() {
      return harness.openExternal;
    },
  },
}));

vi.mock("../desktop.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../desktop.js")>();
  return { ...original, fallbackToPlainNavigation: vi.fn() };
});

import { fallbackToPlainNavigation } from "../desktop.js";
import { DesktopUnavailableCard } from "./DesktopUnavailableCard.js";

function renderCard(apiBaseUrl = "https://api.example.test/api"): void {
  createAppClient(apiBaseUrl, async (url) => ({
    status: 200,
    url,
    headers: { get: () => null },
    text: async () => "{}",
  }));
  render(<DesktopUnavailableCard />);
}

afterEach(() => {
  resetAppClient();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  vi.resetAllMocks();
});

describe("DesktopUnavailableCard", () => {
  it("explains the missing hand-off and focuses its action", () => {
    renderCard();
    expect(screen.getByRole("heading", { name: "The desktop app can't sign you in yet" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Continue in the browser" })).toHaveFocus();
  });

  it("opens the API origin in the system browser", async () => {
    const opened: string[] = [];
    harness.openExternal = async (url: string) => {
      opened.push(url);
    };
    renderCard("https://api.example.test/api/v1");

    await userEvent.setup().click(screen.getByRole("button", { name: "Continue in the browser" }));
    await waitFor(() => expect(opened).toEqual(["https://api.example.test"]));
  });

  it("prefers the baked web base URL", async () => {
    const opened: string[] = [];
    harness.openExternal = async (url: string) => {
      opened.push(url);
    };
    vi.stubGlobal("__PIDASH_WEB_BASE_URL__", "https://app.example.test/");
    renderCard();

    await userEvent.setup().click(screen.getByRole("button", { name: "Continue in the browser" }));
    await waitFor(() => expect(opened).toEqual(["https://app.example.test/"]));
  });

  it("falls back to plain navigation when no native bridge answers", async () => {
    harness.openExternal = async () => {
      throw new Error("no bridge");
    };
    renderCard();

    await userEvent.setup().click(screen.getByRole("button", { name: "Continue in the browser" }));
    await waitFor(() => expect(vi.mocked(fallbackToPlainNavigation)).toHaveBeenCalledWith("https://api.example.test"));
  });

  it("does not navigate when the resolved destination is not a web address", async () => {
    const opened: string[] = [];
    harness.openExternal = async () => {
      opened.push("attempted");
      throw new Error("no bridge");
    };
    renderCard("not-a-url");

    await userEvent.setup().click(screen.getByRole("button", { name: "Continue in the browser" }));
    await waitFor(() => expect(opened).toEqual(["attempted"]));
    expect(vi.mocked(fallbackToPlainNavigation)).not.toHaveBeenCalled();
  });

  it("hides the action without any destination", () => {
    renderCard("");
    expect(screen.queryByRole("button", { name: "Continue in the browser" })).not.toBeInTheDocument();
  });
});
