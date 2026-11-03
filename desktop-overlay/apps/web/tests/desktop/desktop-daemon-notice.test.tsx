/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

type Notice = {
  daemon: {
    mode: string;
    cli_outdated: boolean;
    message: string | null;
    unadopted?: { name: string; workspace: string; reason: string }[];
  } | null;
  cliOutdated: boolean;
  busy: boolean;
};

const mocks = vi.hoisted(() => ({ notice: null as unknown as Notice }));

vi.mock("@/services/desktop-account-sync", () => ({
  getDaemonNotice: () => mocks.notice,
  subscribeDaemonNotice: () => () => {},
  repairSharedDaemon: vi.fn(async () => {}),
  updateCli: vi.fn(async () => {}),
}));

import { DesktopDaemonNotice, unadoptedMessages } from "../../core/components/desktop-daemon-notice";

afterEach(cleanup);

const unadopted = [
  { name: "desktop-a", workspace: "beta", reason: "workspace_mismatch" },
  { name: "desktop-b", workspace: "beta", reason: "workspace_mismatch" },
  { name: "old", workspace: "gamma", reason: "not_signed_in" },
];

describe("runners left in a legacy desktop config", () => {
  it("names them per workspace and what it takes to run them", () => {
    expect(unadoptedMessages(unadopted)).toEqual([
      "Runners desktop-a and desktop-b belong to workspace beta and can't run until the pidash CLI on this computer is signed in to that workspace. Running several workspaces at once is planned for a later update.",
      "Runner old belongs to workspace gamma and can't run until the pidash CLI on this computer is signed in to that workspace. Running several workspaces at once is planned for a later update.",
    ]);
  });

  it("does not promise a sign-in fixes a runner from another server or a clashing config", () => {
    expect(
      unadoptedMessages([
        { name: "far", workspace: "acme", reason: "cloud_mismatch" },
        { name: "dup", workspace: "acme", reason: "invalid_config" },
      ])
    ).toEqual([
      "Runner far from an earlier version of this app is registered with a different Pi Dash server and was not moved to the pidash CLI on this computer.",
      "Runner dup from an earlier version of this app could not be moved to the pidash CLI on this computer: its settings clash with a runner it already has.",
    ]);
  });

  it("shows alongside the daemon notice and can be dismissed", () => {
    mocks.notice = {
      daemon: { mode: "child", cli_outdated: false, message: null, unadopted },
      cliOutdated: false,
      busy: false,
    };
    render(<DesktopDaemonNotice />);
    expect(screen.getByRole("status", { name: "Runners not moved" }).textContent).toContain("workspace beta");
    expect(screen.getByText("Runs only while the app is open.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("status", { name: "Runners not moved" })).toBeNull();
    expect(screen.getByRole("status", { name: "Pi Dash daemon" })).toBeTruthy();
  });

  it("shows on its own when the daemon is fine", () => {
    mocks.notice = {
      daemon: { mode: "service", cli_outdated: false, message: null, unadopted: unadopted.slice(2) },
      cliOutdated: false,
      busy: false,
    };
    render(<DesktopDaemonNotice />);
    expect(screen.getByRole("status", { name: "Runners not moved" })).toBeTruthy();
    expect(screen.queryByRole("status", { name: "Pi Dash daemon" })).toBeNull();
  });

  it("renders nothing when there is nothing to say", () => {
    mocks.notice = { daemon: { mode: "service", cli_outdated: false, message: null }, cliOutdated: false, busy: false };
    const { container } = render(<DesktopDaemonNotice />);
    expect(container.innerHTML).toBe("");
  });
});
