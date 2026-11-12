// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// In-app CLI setup (DESK-021): detect, install when missing, and sign in
// with a grant this session approves — one setup at a time, progress lines
// passed through in order, and the device code (never the typed user code)
// handed to the tool.

import type { ApiClient } from "@pidash/api-client";
import { describe, expect, it, vi } from "vitest";

import type { CliInstallProgress, CliStatus } from "../../core/platform/index.js";
import {
  CLI_SETUP_BUSY,
  CLI_SETUP_UNAVAILABLE,
  createSetupLock,
  setUpCli,
  type CliSetupApi,
  type CliSetupStage,
} from "./cliSetup.js";

const MISSING: CliStatus = { installed: false, path: null, version: null };
const PRESENT: CliStatus = { installed: true, path: "/usr/local/bin/pidash", version: "1.4.0" };

function fakeClient(): ApiClient {
  const sendJson = vi.fn(async (method: string, path: string) => {
    if (path === "/api/v1/auth/device/start/") {
      return {
        status: 200,
        url: path,
        data: {
          device_code: "device-secret",
          user_code: "PIDASH-ABCD",
          verification_uri: "https://app.example.test/auth/device",
          expires_in: 900,
          interval: 5,
        },
      };
    }
    if (path === "/api/v1/auth/device/approve/") {
      return { status: 200, url: path, data: { ok: true, user_email: "a@example.test", workspace_slug: "acme" } };
    }
    throw new Error(`unexpected ${method} ${path}`);
  });
  return { sendJson, parse: (_schema: unknown, data: unknown) => data } as unknown as ApiClient;
}

function fakeCli(overrides: Partial<CliSetupApi> = {}): CliSetupApi {
  let listener: ((progress: CliInstallProgress) => void) | undefined;
  return {
    detect: vi.fn(async () => MISSING),
    install: vi.fn(async () => {
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
}

describe("setUpCli", () => {
  it("installs a missing tool, reports progress and signs it in", async () => {
    const cli = fakeCli({
      install: vi.fn(async () => {
        // The subscription is live while install runs.
        expect(cli.onInstallLog).toHaveBeenCalledTimes(1);
        return PRESENT;
      }),
    });
    const client = fakeClient();
    const lines: string[] = [];
    const stages: CliSetupStage[] = [];
    const result = await setUpCli(
      { client, cloudUrl: "https://app.example.test", workspace: "acme", cli },
      createSetupLock(),
      (progress) => lines.push(progress.line),
      (stage) => stages.push(stage)
    );
    expect(result).toEqual({ status: PRESENT, installedNow: true });
    expect(cli.install).toHaveBeenCalledTimes(1);
    expect(stages).toEqual(["detecting", "installing", "signing-in"]);
    expect(lines).toEqual([]);
    expect(cli.login).toHaveBeenCalledWith({
      deviceCode: "device-secret",
      cloudUrl: "https://app.example.test",
      workspace: "acme",
    });
    expect(client.sendJson).toHaveBeenCalledWith("POST", "/api/v1/auth/device/approve/", {
      json: { user_code: "PIDASH-ABCD" },
    });
  });

  it("streams installer output in order and unsubscribes afterwards", async () => {
    let listener: ((progress: CliInstallProgress) => void) | undefined;
    let unsubscribed = 0;
    const cli = fakeCli({
      onInstallLog: vi.fn((callback) => {
        listener = callback;
        return () => {
          unsubscribed += 1;
        };
      }),
      install: vi.fn(async () => {
        listener?.({ line: "Installing the bundled pidash CLI" });
        listener?.({ line: "Installed /home/me/.local/bin/pidash" });
        return PRESENT;
      }),
    });
    const lines: string[] = [];
    await setUpCli(
      { client: fakeClient(), cloudUrl: "https://app.example.test", workspace: "acme", cli },
      createSetupLock(),
      (progress) => lines.push(progress.line)
    );
    expect(lines).toEqual(["Installing the bundled pidash CLI", "Installed /home/me/.local/bin/pidash"]);
    expect(unsubscribed).toBe(1);
  });

  it("skips the install for a present tool and leaves progress unsubscribed", async () => {
    const cli = fakeCli({ detect: vi.fn(async () => PRESENT) });
    const result = await setUpCli({ client: fakeClient(), cloudUrl: "https://api.test", workspace: "acme", cli });
    expect(result).toEqual({ status: PRESENT, installedNow: false });
    expect(cli.install).not.toHaveBeenCalled();
    expect(cli.onInstallLog).not.toHaveBeenCalled();
  });

  it("releases the lock after a successful setup", async () => {
    const lock = createSetupLock();
    expect(lock.acquire()).toBe(true);
    lock.release();
    expect(lock.acquire()).toBe(true);
  });

  it("refuses a second setup while one runs", async () => {
    let release: (() => void) | undefined;
    const cli = fakeCli({
      detect: vi.fn(
        () =>
          new Promise<CliStatus>((resolve) => {
            release = () => resolve(PRESENT);
          })
      ),
    });
    const lock = createSetupLock();
    const first = setUpCli({ client: fakeClient(), cloudUrl: "https://api.test", workspace: "acme", cli }, lock);
    await expect(
      setUpCli({ client: fakeClient(), cloudUrl: "https://api.test", workspace: "acme", cli }, lock)
    ).rejects.toThrow(CLI_SETUP_BUSY);
    release?.();
    await expect(first).resolves.toMatchObject({ installedNow: false });
  });

  it("releases the lock after a failed setup", async () => {
    const lock = createSetupLock();
    const cli = fakeCli({
      detect: vi.fn(async () => {
        throw new Error("probe failed");
      }),
    });
    await expect(
      setUpCli({ client: fakeClient(), cloudUrl: "https://api.test", workspace: "acme", cli }, lock)
    ).rejects.toThrow("probe failed");
    expect(lock.acquire()).toBe(true);
  });

  it("unsubscribes progress when the install fails", async () => {
    let unsubscribed = 0;
    const cli = fakeCli({
      onInstallLog: vi.fn(() => () => {
        unsubscribed += 1;
      }),
      install: vi.fn(async () => {
        throw new Error("installer failed");
      }),
    });
    await expect(
      setUpCli(
        { client: fakeClient(), cloudUrl: "https://api.test", workspace: "acme", cli },
        createSetupLock(),
        () => undefined
      )
    ).rejects.toThrow("installer failed");
    expect(unsubscribed).toBe(1);
    expect(cli.login).not.toHaveBeenCalled();
  });

  it("does not start a grant when the install is refused", async () => {
    const client = fakeClient();
    const cli = fakeCli({
      install: vi.fn(async () => {
        throw new Error("The command-line tool is already being installed.");
      }),
    });
    await expect(setUpCli({ client, cloudUrl: "https://api.test", workspace: "acme", cli })).rejects.toThrow(
      "already being installed"
    );
    expect(client.sendJson).not.toHaveBeenCalled();
  });

  it("refuses a build without the native tool capability", async () => {
    await expect(
      setUpCli({ client: fakeClient(), cloudUrl: "https://api.test", workspace: "acme", cli: undefined })
    ).rejects.toThrow(CLI_SETUP_UNAVAILABLE);
  });
});
