/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { act, cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  refresh: vi.fn(),
  resume: vi.fn(),
  migrate: vi.fn(),
  register: vi.fn(),
}));

vi.mock("mobx-react", () => ({ observer: (component: unknown) => component }));
vi.mock("@/hooks/store/user", () => ({ useUser: () => ({ data: { id: "user", email: "u@example.com" } }) }));
// The dialogs have their own tests; here they only need to mount.
vi.mock("@/components/desktop-sign-out-dialog", () => ({ DesktopSignOutDialog: () => null }));
vi.mock("@/components/desktop-daemon-setup-dialog", () => ({ DesktopDaemonSetupDialog: () => null }));
vi.mock("@/services/agent-runtime", () => ({
  refreshAgentRuntime: mocks.refresh,
  resumeAgentRuntime: mocks.resume,
  isDesktop: () => true,
}));
vi.mock("@/services/desktop-migration", () => ({ migrateLegacyTrees: mocks.migrate }));
vi.mock("@/services/local-chat-transport", () => ({ registerLocalChatTransport: mocks.register }));

import { AgentRuntime } from "../../core/components/agent-runtime";

beforeEach(() => {
  vi.useFakeTimers();
  vi.resetAllMocks();
  mocks.refresh.mockResolvedValue(undefined);
  mocks.migrate.mockResolvedValue({ status: "nothing_to_do" });
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

async function flush() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe("desktop runtime component", () => {
  it("resumes for the user, installs the chat routing and finds the daemon at launch", async () => {
    render(<AgentRuntime />);
    await flush();
    expect(mocks.resume).toHaveBeenCalledWith("user", "u@example.com");
    expect(mocks.register).toHaveBeenCalledTimes(1);
    expect(mocks.refresh).toHaveBeenCalledTimes(1);
    expect(mocks.migrate).toHaveBeenCalledWith("user");
  });

  it("re-checks the daemon every minute and on focus", async () => {
    render(<AgentRuntime />);
    await flush();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    window.dispatchEvent(new Event("focus"));
    await flush();
    expect(mocks.refresh).toHaveBeenCalledTimes(3);
  });

  it("retries a blocked migration on the next check, and stops once it ran", async () => {
    mocks.migrate.mockResolvedValueOnce({ status: "blocked", reason: "no_daemon" });
    mocks.migrate.mockResolvedValueOnce({ status: "done", migrated: ["acme"], failed: [] });
    render(<AgentRuntime />);
    await flush();
    for (let i = 0; i < 3; i++) {
      // eslint-disable-next-line no-await-in-loop -- one check per minute
      await act(async () => {
        await vi.advanceTimersByTimeAsync(60_000);
      });
    }
    expect(mocks.migrate).toHaveBeenCalledTimes(2);
  });
});
