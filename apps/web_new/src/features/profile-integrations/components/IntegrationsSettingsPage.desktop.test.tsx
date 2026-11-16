// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop install handoff (PIDESKAPP-199): on desktop the GitHub install
// address must open in the system browser through platform.openExternal,
// never navigating the app window. Returning focus to the window re-reads
// the install status once, so completing the install in the browser shows
// up without a manual refresh.
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { GithubAppStatus } from "@pidash/api-client";

const harness = vi.hoisted(() => ({
  location: { searchStr: "", pathname: "/settings/profile/integrations" },
  status: {} as { data: GithubAppStatus | undefined; refetch: ReturnType<typeof vi.fn> },
  install: {} as { mutate: ReturnType<typeof vi.fn>; isPending: boolean },
  refresh: {} as { mutate: ReturnType<typeof vi.fn>; isPending: boolean },
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
  openExternal: vi.fn(async (_url: string): Promise<void> => undefined),
  focusCallbacks: [] as Array<(focused: boolean) => void>,
}));

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useRouterState: ({ select }: { select: (state: { location: typeof harness.location }) => unknown }) =>
    select({ location: harness.location }),
}));

vi.mock("../api.js", () => ({
  useGithubAppStatus: () => harness.status,
  useStartGithubInstall: () => harness.install,
  useRefreshGithubInstall: () => harness.refresh,
}));

vi.mock("../../../shared/shell/toaster.js", () => ({
  notifySuccess: harness.notifySuccess,
  notifyError: harness.notifyError,
}));

vi.mock("../../../core/platform/index.js", () => ({
  get platform() {
    return {
      kind: "desktop",
      openExternal: harness.openExternal,
      onFocusChange: (callback: (focused: boolean) => void) => {
        harness.focusCallbacks.push(callback);
        return () => {
          const index = harness.focusCallbacks.indexOf(callback);
          if (index >= 0) harness.focusCallbacks.splice(index, 1);
        };
      },
    };
  },
}));

import { IntegrationsSettingsPage } from "./IntegrationsSettingsPage.js";

const ID_ONE = "3f7c1d1e-0d1a-4c59-9f0b-6a2f3e4d5c6b";
const ID_TWO = "8a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";

function status(overrides: Partial<GithubAppStatus> = {}): GithubAppStatus {
  return {
    configured: true,
    app_slug: "pi-dash",
    workspaces: [
      { id: ID_ONE, slug: "acme", name: "Acme", github_app: { connected: true, account_login: "octo" } },
      { id: ID_TWO, slug: "beta", name: "Beta", github_app: { connected: false } },
    ],
    ...overrides,
  };
}

let assign: ReturnType<typeof vi.fn>;
let replaceState: ReturnType<typeof vi.spyOn>;

function focus(focused: boolean): void {
  for (const callback of [...harness.focusCallbacks]) callback(focused);
}

beforeEach(() => {
  harness.location = { searchStr: "", pathname: "/settings/profile/integrations" };
  harness.status = { data: status(), refetch: vi.fn() };
  harness.install = { mutate: vi.fn(), isPending: false };
  harness.refresh = { mutate: vi.fn(), isPending: false };
  harness.notifySuccess.mockReset();
  harness.notifyError.mockReset();
  harness.openExternal.mockReset();
  harness.openExternal.mockResolvedValue(undefined);
  harness.focusCallbacks = [];
  assign = vi.fn();
  Object.defineProperty(window, "location", {
    configurable: true,
    value: { ...window.location, assign, hash: "#panel" },
  });
  replaceState = vi.spyOn(window.history, "replaceState").mockImplementation(() => undefined);
});

afterEach(() => replaceState.mockRestore());

describe("IntegrationsSettingsPage on desktop", () => {
  it("opens the install address in the system browser without navigating the window", async () => {
    const user = userEvent.setup();
    harness.install.mutate = vi.fn((_slug: string, options: { onSuccess: (url: string) => void }) => {
      options.onSuccess("https://github.test/install");
    });
    render(<IntegrationsSettingsPage />);
    await user.click(screen.getByRole("button", { name: "Reconnect" }));
    expect(harness.install.mutate).toHaveBeenCalledWith("acme", expect.any(Object));
    expect(harness.openExternal).toHaveBeenCalledWith("https://github.test/install");
    expect(assign).not.toHaveBeenCalled();
  });

  it("re-reads the status once when the window regains focus after a handoff", async () => {
    const user = userEvent.setup();
    harness.install.mutate = vi.fn((_slug: string, options: { onSuccess: (url: string) => void }) => {
      options.onSuccess("https://github.test/install");
    });
    render(<IntegrationsSettingsPage />);
    await user.click(screen.getByRole("button", { name: "Reconnect" }));
    expect(harness.openExternal).toHaveBeenCalledWith("https://github.test/install");
    focus(true);
    expect(harness.status.refetch).toHaveBeenCalledTimes(1);
    focus(true);
    expect(harness.status.refetch).toHaveBeenCalledTimes(1);
  });

  it("does not re-read on focus when no handoff is pending", async () => {
    render(<IntegrationsSettingsPage />);
    focus(true);
    expect(harness.status.refetch).not.toHaveBeenCalled();
  });

  it("surfaces an error when the system browser cannot open", async () => {
    const user = userEvent.setup();
    harness.openExternal.mockRejectedValue(new Error("blocked"));
    harness.install.mutate = vi.fn((_slug: string, options: { onSuccess: (url: string) => void }) => {
      options.onSuccess("https://github.test/install");
    });
    render(<IntegrationsSettingsPage />);
    await user.click(screen.getByRole("button", { name: "Reconnect" }));
    await waitFor(() => expect(harness.notifyError).toHaveBeenCalled());
    expect(harness.notifyError).toHaveBeenCalledWith("Install failed", expect.any(String));
    expect(assign).not.toHaveBeenCalled();
  });
});
