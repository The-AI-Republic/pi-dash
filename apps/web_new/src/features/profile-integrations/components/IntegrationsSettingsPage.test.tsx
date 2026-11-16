// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Integrations tab body (parity/specs/profile.md §11, PROF-025): the
// admin workspace picker, the member's no-admin view, the install handoff
// leaving the app, and the return marker becoming one notice, one
// selection and a scrubbed address while unrelated parameters stay.
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { GithubAppStatus } from "@pidash/api-client";

const harness = vi.hoisted(() => ({
  location: { searchStr: "" as string, pathname: "/settings/profile/integrations" },
  status: {} as { data: GithubAppStatus | undefined; refetch: ReturnType<typeof vi.fn> },
  install: {} as { mutate: ReturnType<typeof vi.fn>; isPending: boolean },
  refresh: {} as { mutate: ReturnType<typeof vi.fn>; isPending: boolean },
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
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

beforeEach(() => {
  harness.location = { searchStr: "", pathname: "/settings/profile/integrations" };
  harness.status = { data: status(), refetch: vi.fn() };
  harness.install = { mutate: vi.fn(), isPending: false };
  harness.refresh = { mutate: vi.fn(), isPending: false };
  harness.notifySuccess.mockReset();
  harness.notifyError.mockReset();
  assign = vi.fn();
  Object.defineProperty(window, "location", {
    configurable: true,
    value: { ...window.location, assign, hash: "#panel" },
  });
  replaceState = vi.spyOn(window.history, "replaceState").mockImplementation(() => undefined);
});

afterEach(() => replaceState.mockRestore());

describe("IntegrationsSettingsPage", () => {
  it("holds a quiet frame before the status read", () => {
    harness.status = { data: undefined, refetch: vi.fn() };
    render(<IntegrationsSettingsPage />);
    expect(screen.getByTestId("integrations-settings-loading")).toHaveAttribute("aria-busy", "true");
    expect(screen.queryByTestId("github-app-panel")).not.toBeInTheDocument();
  });

  it("explains a member with no admin workspace", () => {
    harness.status = { data: status({ workspaces: [] }), refetch: vi.fn() };
    render(<IntegrationsSettingsPage />);
    expect(screen.getByTestId("integrations-no-admin")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "No admin workspaces" })).toBeInTheDocument();
    expect(screen.queryByLabelText("Workspace")).not.toBeInTheDocument();
    expect(screen.queryByTestId("github-app-panel")).not.toBeInTheDocument();
  });

  it("opens on the first admin workspace and switches on demand", async () => {
    const user = userEvent.setup();
    render(<IntegrationsSettingsPage />);
    const picker = screen.getByLabelText("Workspace");
    expect(picker).toHaveValue("acme");
    expect(within(screen.getByTestId("github-app-panel")).getByText("Installed for octo on Acme.")).toBeInTheDocument();
    await user.selectOptions(picker, "beta");
    expect(
      within(screen.getByTestId("github-app-panel")).getByText("Install the Pi Dash GitHub App for Beta.")
    ).toBeInTheDocument();
  });

  it("hands the browser to the host's install address", async () => {
    const user = userEvent.setup();
    harness.install.mutate = vi.fn((_slug: string, options: { onSuccess: (url: string) => void }) => {
      options.onSuccess("https://github.test/install");
    });
    render(<IntegrationsSettingsPage />);
    await user.click(screen.getByRole("button", { name: "Reconnect" }));
    expect(harness.install.mutate).toHaveBeenCalledWith("acme", expect.any(Object));
    expect(assign).toHaveBeenCalledWith("https://github.test/install");
  });

  it("asks its own hook to re-check the shown workspace", async () => {
    const user = userEvent.setup();
    render(<IntegrationsSettingsPage />);
    await user.click(screen.getByRole("button", { name: "Refresh" }));
    expect(harness.refresh.mutate).toHaveBeenCalledWith("acme");
  });

  it("turns a success callback into a notice, a selection, a re-read and a scrubbed address", async () => {
    harness.location = {
      searchStr: "?github_app=connected&workspace_slug=beta&foo=bar",
      pathname: "/settings/profile/integrations",
    };
    render(<IntegrationsSettingsPage />);
    await waitFor(() =>
      expect(harness.notifySuccess).toHaveBeenCalledWith(
        "GitHub connected",
        "The GitHub App installation was verified."
      )
    );
    expect(screen.getByLabelText("Workspace")).toHaveValue("beta");
    expect(harness.status.refetch).toHaveBeenCalledTimes(1);
    expect(replaceState).toHaveBeenCalledWith(null, "", "/settings/profile/integrations?foo=bar#panel");
  });

  it("words a failed callback and still scrubs it", async () => {
    harness.location = {
      searchStr: "?github_app=error&error=login_required&foo=bar",
      pathname: "/settings/profile/integrations",
    };
    render(<IntegrationsSettingsPage />);
    await waitFor(() =>
      expect(harness.notifyError).toHaveBeenCalledWith(
        "GitHub connection failed",
        "Sign in to Pi Dash and start the GitHub App install again."
      )
    );
    expect(harness.status.refetch).not.toHaveBeenCalled();
    expect(replaceState).toHaveBeenCalledWith(null, "", "/settings/profile/integrations?foo=bar#panel");
  });

  it("uses the fallback wording for an unknown callback failure", async () => {
    harness.location = { searchStr: "?github_app=mystery", pathname: "/x" };
    render(<IntegrationsSettingsPage />);
    await waitFor(() =>
      expect(harness.notifyError).toHaveBeenCalledWith(
        "GitHub connection failed",
        "The GitHub App installation could not be completed."
      )
    );
  });

  it("notices and scrubs an install callback only once", async () => {
    harness.location = { searchStr: "?github_app=connected&workspace_slug=beta", pathname: "/x" };
    const { rerender } = render(<IntegrationsSettingsPage />);
    await waitFor(() => expect(harness.notifySuccess).toHaveBeenCalledTimes(1));
    harness.status = { data: harness.status.data, refetch: harness.status.refetch };
    rerender(<IntegrationsSettingsPage />);
    expect(harness.notifySuccess).toHaveBeenCalledTimes(1);
    expect(replaceState).toHaveBeenCalledTimes(1);
  });

  it("leaves an address without a marker untouched", () => {
    harness.location = { searchStr: "?foo=bar", pathname: "/x" };
    render(<IntegrationsSettingsPage />);
    expect(harness.notifySuccess).not.toHaveBeenCalled();
    expect(harness.notifyError).not.toHaveBeenCalled();
    expect(replaceState).not.toHaveBeenCalled();
  });

  it("passes the panel's in-flight flags through", () => {
    harness.install = { mutate: vi.fn(), isPending: true };
    harness.refresh = { mutate: vi.fn(), isPending: true };
    render(<IntegrationsSettingsPage />);
    expect(screen.getByRole("button", { name: "Reconnect" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Refresh" })).toBeDisabled();
  });

  it("binds no shortcuts, drops or file inputs (PROF-027)", async () => {
    const user = userEvent.setup();
    const { container } = render(<IntegrationsSettingsPage />);
    expect(container.querySelectorAll('input[type="file"]')).toHaveLength(0);
    expect(container.querySelectorAll("[draggable='true']")).toHaveLength(0);
    await user.keyboard("{Control>}r{/Control}");
    expect(harness.install.mutate).not.toHaveBeenCalled();
    expect(harness.refresh.mutate).not.toHaveBeenCalled();
  });

  it("disables the handoff while the instance is unconfigured", () => {
    harness.status = { data: status({ configured: false }), refetch: vi.fn() };
    render(<IntegrationsSettingsPage />);
    expect(screen.getByRole("button", { name: "Reconnect" })).toBeDisabled();
  });
});
