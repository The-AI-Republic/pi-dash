/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ toast: vi.fn() }));

vi.mock("@/services/agent-runtime", () => ({ isDesktop: () => true }));
vi.mock("@pi-dash/propel/toast", () => ({ TOAST_TYPE: { ERROR: "error" }, setToast: mocks.toast }));
vi.mock("@pi-dash/propel/tooltip", () => ({
  Tooltip: ({ children }: { children: React.ReactNode }) => children,
}));

import { DesktopUpdateButton, UPDATE_AVAILABLE_EVENT } from "../../core/components/desktop-update-button";

type Handler = (e: { payload: unknown }) => void;
let listeners: Map<string, Handler>;
let invoke: ReturnType<typeof vi.fn>;

beforeEach(() => {
  listeners = new Map();
  invoke = vi.fn().mockResolvedValue(null);
  const listen = vi.fn((event: string, handler: Handler) => {
    listeners.set(event, handler);
    return Promise.resolve(() => listeners.delete(event));
  });
  Object.assign(window, { __TAURI__: { core: { invoke }, event: { listen } } });
});

afterEach(() => {
  cleanup();
  Reflect.deleteProperty(window, "__TAURI__");
  mocks.toast.mockReset();
});

const flush = () => act(async () => {});

describe("DesktopUpdateButton", () => {
  it("renders nothing while no update is waiting", async () => {
    render(<DesktopUpdateButton />);
    await flush();
    expect(invoke).toHaveBeenCalledWith("desktop_pending_update");
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("shows an update deferred before it mounted", async () => {
    invoke.mockResolvedValueOnce({ version: "0.4.0", currentVersion: "0.3.2" });
    render(<DesktopUpdateButton />);
    await flush();
    expect(screen.getByRole("button", { name: "Update to Pi Dash 0.4.0" })).toBeTruthy();
  });

  it("appears when the daily check announces an update", async () => {
    render(<DesktopUpdateButton />);
    await flush();
    act(() => listeners.get(UPDATE_AVAILABLE_EVENT)?.({ payload: { version: "0.4.0", currentVersion: "0.3.2" } }));
    expect(screen.getByRole("button", { name: "Update to Pi Dash 0.4.0" })).toBeTruthy();
  });

  it("installs on click and reports a failure", async () => {
    invoke.mockResolvedValueOnce({ version: "0.4.0", currentVersion: "0.3.2" });
    render(<DesktopUpdateButton />);
    await flush();
    invoke.mockRejectedValueOnce("signature mismatch");
    fireEvent.click(screen.getByRole("button"));
    await flush();
    expect(invoke).toHaveBeenLastCalledWith("desktop_install_update");
    expect(mocks.toast).toHaveBeenCalledWith(expect.objectContaining({ message: "signature mismatch" }));
    expect((screen.getByRole("button") as HTMLButtonElement).disabled).toBe(false);
  });

  it("stops listening on unmount", async () => {
    const { unmount } = render(<DesktopUpdateButton />);
    await flush();
    unmount();
    expect(listeners.has(UPDATE_AVAILABLE_EVENT)).toBe(false);
  });
});
