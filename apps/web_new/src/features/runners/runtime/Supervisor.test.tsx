// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  me: { data: { id: "u1", email: "u1@test.dev" } as { id: string; email: string } | undefined },
  desktop: true,
  focusListeners: [] as Array<(focused: boolean) => void>,
  unsubscribe: vi.fn(),
  resume: vi.fn(),
  refresh: vi.fn(),
}));

vi.mock("../../../core/platform/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../core/platform/index.js")>();
  const desktopPlatform = {
    kind: "desktop",
    apiOrigin: "https://cloud.test",
    agentRuntime: {},
    onFocusChange: (callback: (focused: boolean) => void) => {
      harness.focusListeners.push(callback);
      return harness.unsubscribe;
    },
  };
  return {
    ...original,
    get platform() {
      return harness.desktop ? desktopPlatform : original.platform;
    },
  };
});

vi.mock("../../../core/session/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../core/session/index.js")>();
  return { ...original, useMe: () => harness.me };
});

vi.mock("./service.js", () => ({
  resumeRuntime: (...args: Array<unknown>) => harness.resume(...args),
  refreshRuntime: (...args: Array<unknown>) => harness.refresh(...args),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { RUNTIME_REFRESH_INTERVAL_MS, RuntimeSupervisor } from "./Supervisor.js";

function SetupStub(): React.ReactElement {
  return <div data-testid="setup-host" />;
}

beforeEach(() => {
  vi.clearAllMocks();
  harness.me = { data: { id: "u1", email: "u1@test.dev" } };
  harness.desktop = true;
  harness.focusListeners = [];
  harness.refresh.mockResolvedValue(undefined);
  createAppClient("https://api.test");
});

afterEach(() => {
  resetAppClient();
  vi.useRealTimers();
});

function renderSupervisor(props: { registerChatTransport?: () => void } = {}) {
  return render(
    <RuntimeSupervisor registerChatTransport={props.registerChatTransport ?? vi.fn()} setupDialog={SetupStub} />
  );
}

function focusApp(focused: boolean): void {
  for (const listener of harness.focusListeners) listener(focused);
}

describe("RuntimeSupervisor", () => {
  it("renders nothing on web and runs no step", () => {
    harness.desktop = false;
    const { container } = renderSupervisor();
    expect(container).toBeEmptyDOMElement();
    expect(harness.resume).not.toHaveBeenCalled();
    expect(harness.refresh).not.toHaveBeenCalled();
  });

  it("resumes for the signed account against the app's cloud and hosts the set-up dialog", async () => {
    const register = vi.fn();
    renderSupervisor({ registerChatTransport: register });
    expect(await screen.findByTestId("setup-host")).toBeInTheDocument();
    expect(harness.resume).toHaveBeenCalledWith(
      { id: "u1", email: "u1@test.dev", cloudUrl: "https://cloud.test" },
      expect.objectContaining({ runtime: expect.anything() })
    );
    expect(register).toHaveBeenCalledTimes(1);
    expect(harness.refresh).toHaveBeenCalledTimes(1);
  });

  it("shows no project notice: opening a project enrols nothing", async () => {
    renderSupervisor();
    await screen.findByTestId("setup-host");
    expect(screen.queryByTestId("runtime-notice")).toBeNull();
  });

  it("re-checks on the minute cadence and on focus, not on blur", async () => {
    vi.useFakeTimers();
    renderSupervisor();
    await act(async () => undefined);
    expect(harness.refresh).toHaveBeenCalledTimes(1);
    await act(async () => {
      vi.advanceTimersByTime(RUNTIME_REFRESH_INTERVAL_MS);
    });
    expect(harness.refresh).toHaveBeenCalledTimes(2);
    act(() => focusApp(false));
    expect(harness.refresh).toHaveBeenCalledTimes(2);
    act(() => focusApp(true));
    expect(harness.refresh).toHaveBeenCalledTimes(3);
  });

  it("swallows a failed re-check", async () => {
    harness.refresh.mockRejectedValue(new Error("boom"));
    renderSupervisor();
    await screen.findByTestId("setup-host");
    await act(async () => undefined);
  });

  it("cleans up timers and focus listeners on unmount", async () => {
    vi.useFakeTimers();
    const view = renderSupervisor();
    await act(async () => undefined);
    view.unmount();
    expect(harness.unsubscribe).toHaveBeenCalledTimes(1);
    await act(async () => {
      vi.advanceTimersByTime(RUNTIME_REFRESH_INTERVAL_MS * 2);
    });
    expect(harness.refresh).toHaveBeenCalledTimes(1);
  });

  it("stays quiet while signed out", () => {
    harness.me = { data: undefined };
    const { container } = renderSupervisor();
    expect(container).toBeEmptyDOMElement();
    expect(harness.resume).not.toHaveBeenCalled();
  });
});
