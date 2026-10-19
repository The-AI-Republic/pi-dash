// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// CLI setup panel (DESK-021): no render without the native capability, the
// installed version in the summary line, install output as plain text in
// order, the success state, and a failure that keeps the panel usable.

import { ToastHost } from "@pidash/kit";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ApiClient } from "@pidash/api-client";

import type { CliInstallProgress, CliSetupApi, Platform } from "../../../core/platform/index.js";
import { getToastManager } from "../../../shared/shell/toaster.js";

const platformHarness = vi.hoisted(() => ({ current: {} as Partial<Platform> }));
const clientHarness = vi.hoisted(() => ({ current: null as unknown }));
vi.mock("../../../core/api/client.js", () => ({
  getAppClient: () => clientHarness.current,
}));
vi.mock("../../../core/platform/index.js", () => ({
  get platform() {
    return platformHarness.current;
  },
}));

import { CLI_SETUP_HEADING, CliSetupPanel, describeCliStatus } from "./CliSetupPanel.js";

const PRESENT = { installed: true, path: "/usr/local/bin/pidash", version: "2.0.0" };
const MISSING = { installed: false, path: null, version: null };

function installCli(overrides: Partial<CliSetupApi> = {}): {
  cli: CliSetupApi;
  emit: (progress: CliInstallProgress) => void;
} {
  let listener: ((progress: CliInstallProgress) => void) | undefined;
  const cli: CliSetupApi = {
    detect: vi.fn(async () => MISSING),
    install: vi.fn(async () => {
      listener?.({ line: "Downloading" });
      listener?.({ line: "Installed" });
      return PRESENT;
    }),
    onInstallLog: vi.fn((callback) => {
      listener = callback;
      return () => {
        listener = undefined;
      };
    }),
    login: vi.fn(async () => undefined),
    ...overrides,
  };
  return {
    cli,
    emit: (progress) => listener?.(progress),
  };
}

function stubClients(): void {
  const sendJson = vi.fn(async (_method: string, path: string) => {
    if (path === "/api/v1/auth/device/start/") {
      return {
        status: 200,
        url: path,
        data: {
          device_code: "device-secret",
          user_code: "PIDASH-CODE",
          verification_uri: "https://app.test/auth/device",
          expires_in: 900,
          interval: 5,
        },
      };
    }
    return { status: 200, url: path, data: { ok: true, user_email: "a@test", workspace_slug: "acme" } };
  });
  clientHarness.current = {
    sendJson,
    baseUrl: "https://app.test",
    parse: (_schema: unknown, data: unknown) => data,
  } as unknown as ApiClient;
}

function renderPanel(): { unmount: () => void } {
  return render(
    <>
      <ToastHost manager={getToastManager()} />
      <CliSetupPanel workspaceSlug="acme" />
    </>
  );
}

afterEach(() => {
  platformHarness.current = {};
  vi.clearAllMocks();
});

describe("describeCliStatus", () => {
  it("names each state", () => {
    expect(describeCliStatus(null, true)).toContain("Checking");
    expect(describeCliStatus(null, false)).toBe("Not checked yet.");
    expect(describeCliStatus(MISSING, false)).toBe("Not installed on this machine.");
    expect(describeCliStatus({ ...PRESENT, version: null }, false)).toBe("Installed.");
    expect(describeCliStatus(PRESENT, false)).toBe("Installed, version 2.0.0.");
  });
});

describe("CliSetupPanel", () => {
  it("renders nothing without the native tool capability", () => {
    platformHarness.current = {};
    renderPanel();
    expect(screen.queryByRole("region", { name: CLI_SETUP_HEADING })).not.toBeInTheDocument();
  });

  it("reports the installed version found on mount", async () => {
    const { cli } = installCli({ detect: vi.fn(async () => PRESENT) });
    platformHarness.current = { cliSetup: cli };
    renderPanel();
    expect(screen.getByRole("heading", { name: CLI_SETUP_HEADING })).toBeInTheDocument();
    await waitFor(() => expect(screen.getByTestId("cli-setup-status")).toHaveTextContent("version 2.0.0"));
    expect(screen.getByRole("button", { name: "Sign in the tool" })).toBeInTheDocument();
  });

  it("shows install output lines as plain text in order, then the ready state", async () => {
    stubClients();
    const user = userEvent.setup();
    let finishInstall: ((status: typeof PRESENT) => void) | undefined;
    const { cli, emit } = installCli({
      install: vi.fn(
        () =>
          new Promise<typeof PRESENT>((resolve) => {
            finishInstall = resolve;
          })
      ),
    });
    platformHarness.current = { cliSetup: cli };
    renderPanel();
    await waitFor(() => expect(screen.getByTestId("cli-setup-status")).toHaveTextContent("Not installed"));
    await user.click(screen.getByRole("button", { name: "Set up the tool" }));
    await waitFor(() => expect(screen.getByTestId("cli-setup-stage")).toHaveTextContent("Installing"));
    act(() => {
      emit({ line: "<b>not markup</b>" });
      emit({ line: "second" });
    });
    const log = screen.getByTestId("cli-setup-log");
    expect(log.tagName).toBe("PRE");
    expect(log.textContent).toBe("<b>not markup</b>\nsecond");
    await act(async () => {
      finishInstall?.(PRESENT);
    });
    await waitFor(() => expect(screen.getByTestId("cli-setup-status")).toHaveTextContent("version 2.0.0"));
    expect(screen.queryByTestId("cli-setup-stage")).not.toBeInTheDocument();
  });

  it("surfaces a failure in the panel and a toast, then allows a retry", async () => {
    stubClients();
    const user = userEvent.setup();
    const { cli } = installCli({
      detect: vi.fn(async () => PRESENT),
      login: vi.fn(async () => {
        throw new Error("The tool refused the code.");
      }),
    });
    platformHarness.current = { cliSetup: cli };
    renderPanel();
    await waitFor(() => expect(screen.getByRole("button", { name: "Sign in the tool" })).toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Sign in the tool" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("The tool refused the code.");
    expect(screen.getByText("Couldn't set up the command-line tool")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sign in the tool" })).toBeEnabled();
  });

  it("reports a rejected probe in the panel without a toast", async () => {
    stubClients();
    const { cli } = installCli({
      detect: vi.fn(async () => {
        throw new Error("The probe was refused.");
      }),
    });
    platformHarness.current = { cliSetup: cli };
    renderPanel();
    expect(await screen.findByRole("alert")).toHaveTextContent("The probe was refused.");
    expect(screen.getByTestId("cli-setup-status")).toHaveTextContent("Not checked yet.");
    expect(screen.getByRole("button", { name: "Set up the tool" })).toBeEnabled();
  });

  it("drops a probe answer that lands after the panel unmounts", async () => {
    stubClients();
    let resolveDetect: ((status: typeof PRESENT) => void) | undefined;
    let rejectDetect: ((failure: Error) => void) | undefined;
    const detect = vi
      .fn<() => Promise<typeof PRESENT>>()
      .mockImplementationOnce(
        () =>
          new Promise<typeof PRESENT>((resolve) => {
            resolveDetect = resolve;
          })
      )
      .mockImplementationOnce(
        () =>
          new Promise<typeof PRESENT>((_resolve, reject) => {
            rejectDetect = reject;
          })
      );
    const { cli } = installCli({ detect: detect as unknown as CliSetupApi["detect"] });
    platformHarness.current = { cliSetup: cli };
    const first = renderPanel();
    first.unmount();
    await act(async () => {
      resolveDetect?.(PRESENT);
    });
    const second = renderPanel();
    second.unmount();
    await act(async () => {
      rejectDetect?.(new Error("late failure"));
    });
    expect(detect).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("uses a general message for a rejected non-Error probe", async () => {
    stubClients();
    const { cli } = installCli({
      detect: vi.fn(async () => {
        throw "boom";
      }),
    });
    platformHarness.current = { cliSetup: cli };
    renderPanel();
    expect(await screen.findByRole("alert")).toHaveTextContent("Something went wrong while setting up the tool.");
  });
});
