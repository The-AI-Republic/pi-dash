// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop hand-off surface (M-01): the app window opens an address it does
// not own on the hosted web app, says so, and explains itself (with a
// retry) when the browser cannot be opened.

import * as React from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  kind: "desktop" as "desktop" | "web",
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

import {
  DesktopHostedScreen,
  HOSTED_SCREEN_FAILURE_HEADING,
  HOSTED_SCREEN_HEADING,
  HOSTED_SCREEN_NO_ADDRESS,
  HOSTED_SCREEN_OPENED,
  HOSTED_SCREEN_REFUSED,
  HOSTED_SCREEN_RETRY,
  HOSTED_SCREEN_TITLE,
  hostedScreenAddressFor,
} from "./DesktopHostedScreen.js";

beforeEach(() => {
  harness.kind = "desktop";
  harness.openExternal.mockReset();
  harness.openExternal.mockImplementation(async () => undefined);
  window.history.replaceState(null, "", "/acme/projects/archives?tab=modules#archived-1");
});

afterEach(() => {
  vi.unstubAllGlobals();
});

function bakeWebBase(value: string): void {
  vi.stubGlobal("__PIDASH_WEB_BASE_URL__", value);
}

describe("DesktopHostedScreen", () => {
  it("opens the hosted address for the current path, query and fragment", async () => {
    bakeWebBase("https://app.example.test");
    render(<DesktopHostedScreen />);

    await waitFor(() =>
      expect(harness.openExternal).toHaveBeenCalledWith(
        "https://app.example.test/acme/projects/archives?tab=modules#archived-1"
      )
    );
    expect(await screen.findByText(HOSTED_SCREEN_OPENED)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: HOSTED_SCREEN_HEADING })).toBeInTheDocument();
    expect(document.title).toBe(HOSTED_SCREEN_TITLE);
    // Exactly one hand-off for one attempt, however often React renders.
    expect(harness.openExternal).toHaveBeenCalledTimes(1);
  });

  it("hands the address over once under React's double-invoked development effects", async () => {
    bakeWebBase("https://app.example.test");
    render(
      <React.StrictMode>
        <DesktopHostedScreen />
      </React.StrictMode>
    );
    expect(await screen.findByText(HOSTED_SCREEN_OPENED)).toBeInTheDocument();
    expect(harness.openExternal).toHaveBeenCalledTimes(1);
  });

  it("hands the address over once however often it re-renders", async () => {
    bakeWebBase("https://app.example.test");
    const { rerender } = render(<DesktopHostedScreen />);
    await waitFor(() => expect(harness.openExternal).toHaveBeenCalledTimes(1));
    rerender(<DesktopHostedScreen />);
    expect(harness.openExternal).toHaveBeenCalledTimes(1);
    expect(await screen.findByText(HOSTED_SCREEN_OPENED)).toBeInTheDocument();
  });

  it("opens the new address when the surface stays mounted across a route change", async () => {
    bakeWebBase("https://app.example.test");
    const { rerender } = render(<DesktopHostedScreen />);
    await waitFor(() => expect(harness.openExternal).toHaveBeenCalledTimes(1));
    window.history.replaceState(null, "", "/acme/notifications");
    rerender(<DesktopHostedScreen />);
    await waitFor(() =>
      expect(harness.openExternal).toHaveBeenLastCalledWith("https://app.example.test/acme/notifications")
    );
    expect(harness.openExternal).toHaveBeenCalledTimes(2);
  });

  it("says so and offers no retry when the build has no web address", async () => {
    render(<DesktopHostedScreen />);

    expect(await screen.findByRole("heading", { name: HOSTED_SCREEN_FAILURE_HEADING })).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent(HOSTED_SCREEN_NO_ADDRESS);
    expect(screen.queryByRole("button", { name: HOSTED_SCREEN_RETRY })).toBeNull();
    expect(harness.openExternal).not.toHaveBeenCalled();
  });

  it("reports a refusal with the address and retries it", async () => {
    bakeWebBase("https://app.example.test");
    harness.openExternal.mockRejectedValueOnce(new Error("The operating system refused to open the browser."));
    render(<DesktopHostedScreen />);

    expect(await screen.findByRole("heading", { name: HOSTED_SCREEN_FAILURE_HEADING })).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("The operating system refused to open the browser.");
    expect(
      screen.getByText("https://app.example.test/acme/projects/archives?tab=modules#archived-1")
    ).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: HOSTED_SCREEN_RETRY }));
    expect(await screen.findByText(HOSTED_SCREEN_OPENED)).toBeInTheDocument();
    expect(harness.openExternal).toHaveBeenCalledTimes(2);
  });

  it("falls back to its own wording when the refusal carries no message", async () => {
    bakeWebBase("https://app.example.test");
    harness.openExternal.mockRejectedValueOnce("no bridge");
    render(<DesktopHostedScreen />);

    expect(await screen.findByRole("alert")).toHaveTextContent(HOSTED_SCREEN_REFUSED);
  });

  it("keeps crawlers out of the hand-off surface", async () => {
    bakeWebBase("https://app.example.test");
    render(<DesktopHostedScreen />);
    await waitFor(() => expect(harness.openExternal).toHaveBeenCalled());
    expect(document.querySelector('meta[name="robots"]')?.getAttribute("content")).toBe("noindex, nofollow");
  });
});

describe("hostedScreenAddressFor", () => {
  it("resolves against the baked origin", () => {
    bakeWebBase("https://app.example.test/");
    expect(hostedScreenAddressFor("/acme/projects/archives")).toBe("https://app.example.test/acme/projects/archives");
  });

  it("is null without a baked origin", () => {
    expect(hostedScreenAddressFor("/acme/projects/archives")).toBeNull();
  });

  it("is null when the baked value is not a web address", () => {
    bakeWebBase("tauri://localhost");
    expect(hostedScreenAddressFor("/acme/projects/archives")).toBeNull();
  });
});
