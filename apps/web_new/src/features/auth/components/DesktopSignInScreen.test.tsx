// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const harness = vi.hoisted(() => ({
  edition: { id: "oss", flags: {} } as { id: string; flags: Record<string, boolean>; slots?: Record<string, unknown> },
  openExternal: vi.fn(async (_url: string): Promise<void> => undefined),
}));

vi.mock("../../../core/edition/index.js", () => ({
  get edition() {
    return harness.edition;
  },
}));

vi.mock("../../../core/platform/index.js", () => ({
  platform: {
    get openExternal() {
      return harness.openExternal;
    },
  },
}));

import { DesktopSignInScreen } from "./DesktopSignInScreen.js";

function renderScreen(props: { error?: string | undefined; errorDescription?: string | undefined } = {}): void {
  createAppClient("https://api.example.test", async (url) => ({
    status: 200,
    url,
    headers: { get: () => null },
    text: async () => "{}",
  }));
  render(<DesktopSignInScreen error={props.error} errorDescription={props.errorDescription} />);
}

afterEach(() => {
  resetAppClient();
  harness.edition = { id: "oss", flags: {} };
  harness.openExternal.mockClear();
});

describe("DesktopSignInScreen", () => {
  it("renders the unavailable card without an edition card", () => {
    renderScreen();
    expect(screen.getByRole("heading", { name: "The desktop app can't sign you in yet" })).toBeVisible();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("renders the edition card when one is slotted", () => {
    harness.edition = {
      id: "cloud",
      flags: {},
      slots: { desktopSignInCard: () => <p>Edition sign-in card</p> },
    };
    renderScreen();
    expect(screen.getByText("Edition sign-in card")).toBeVisible();
    expect(screen.queryByRole("heading", { name: "The desktop app can't sign you in yet" })).not.toBeInTheDocument();
  });

  it("shows the provider description above the card", () => {
    renderScreen({ error: "access_denied", errorDescription: "The provider refused the flow." });
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Could not finish signing in");
    expect(alert).toHaveTextContent("The provider refused the flow.");
    expect(screen.getByRole("heading", { name: "The desktop app can't sign you in yet" })).toBeVisible();
  });

  it("names the error when no description arrived", () => {
    renderScreen({ error: "access_denied" });
    expect(screen.getByRole("alert")).toHaveTextContent("access_denied");
  });

  it("never starts sign-in on its own", () => {
    renderScreen({ error: "access_denied" });
    expect(harness.openExternal).not.toHaveBeenCalled();
  });
});
