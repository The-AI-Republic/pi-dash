// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar update control (DESK-018): nothing while current, both versions
// named, disabled while downloading, in-place failure with a working retry.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { PendingUpdate } from "../../core/platform/index.js";
import {
  DesktopUpdateControl,
  DesktopUpdateControlView,
  updateControlName,
  updateInstallingHint,
  updateReadyHint,
} from "./DesktopUpdateControl.js";
import type { PendingUpdate as PlatformPendingUpdate, UpdaterApi } from "../../core/platform/index.js";
import { UPDATE_NO_RESTART_TEXT, type UpdaterState } from "./state.js";

// The connected control reads its capability from the platform; the hook's
// own behavior is covered in state.test.tsx.
const harness = vi.hoisted(() => ({
  capability: undefined as UpdaterApi | undefined,
}));

vi.mock("../../core/platform/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../core/platform/index.js")>();
  return {
    ...original,
    platform: {
      get updates() {
        return harness.capability;
      },
    },
  };
});

const WAITING: PendingUpdate = { version: "2.0.0", currentVersion: "1.9.4", installing: false };

function state(overrides: Partial<UpdaterState> = {}): UpdaterState {
  return { update: WAITING, requesting: false, failure: null, install: vi.fn(), ...overrides };
}

function view(value: UpdaterState): React.ReactElement {
  return <DesktopUpdateControlView state={value} />;
}

describe("DesktopUpdateControlView", () => {
  it("renders nothing while no update waits", () => {
    const { container } = render(view(state({ update: null })));
    expect(container).toBeEmptyDOMElement();
  });

  it("names the waiting release and exposes the current one", async () => {
    const user = userEvent.setup();
    render(view(state()));
    const control = screen.getByRole("button", { name: "Update to Pi Dash 2.0.0" });
    expect(control).toBeEnabled();
    expect(control).not.toHaveAttribute("aria-busy");
    expect(control).toHaveTextContent("Update to Pi Dash 2.0.0");
    expect(control).toHaveAccessibleDescription("You are on 1.9.4");
    await user.hover(control);
    expect(await screen.findByText(updateReadyHint("2.0.0", "1.9.4"))).toBeInTheDocument();
    expect(updateReadyHint("2.0.0", "1.9.4")).toContain("1.9.4");
  });

  it("installs on click", async () => {
    const user = userEvent.setup();
    const value = state();
    render(view(value));
    await user.click(screen.getByRole("button", { name: "Update to Pi Dash 2.0.0" }));
    expect(value.install).toHaveBeenCalledTimes(1);
  });

  it("disables itself while this page's install runs", () => {
    render(view(state({ requesting: true })));
    const control = screen.getByRole("button", { name: "Update to Pi Dash 2.0.0" });
    expect(control).toBeDisabled();
    expect(control).toHaveAttribute("aria-busy", "true");
    expect(screen.getByText(updateInstallingHint("2.0.0"))).toBeInTheDocument();
  });

  it("reads disabled when it mounts mid-download", () => {
    // The shell's installing flag, not page memory, decides this state.
    render(view(state({ update: { ...WAITING, installing: true } })));
    expect(screen.getByRole("button", { name: "Update to Pi Dash 2.0.0" })).toBeDisabled();
  });

  it("reports a failure in place with a retry that installs again", async () => {
    const user = userEvent.setup();
    const value = state({ failure: "signature mismatch" });
    render(view(value));
    expect(screen.getByRole("alert")).toHaveTextContent("signature mismatch");
    // The failed release stays visible and actionable.
    expect(screen.getByRole("button", { name: "Update to Pi Dash 2.0.0" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(value.install).toHaveBeenCalledTimes(1);
  });

  it("builds the accessible name and hints from the versions", () => {
    expect(updateControlName("3.1.0")).toBe("Update to Pi Dash 3.1.0");
    expect(updateReadyHint("3.1.0", "3.0.9")).toContain("3.1.0");
    expect(updateReadyHint("3.1.0", "3.0.9")).toContain("3.0.9");
    expect(updateInstallingHint("3.1.0")).toBe("Installing 3.1.0…");
  });
});

describe("DesktopUpdateControl", () => {
  it("renders nothing on a platform without updates (web bundle)", async () => {
    harness.capability = undefined;
    const { container } = render(<DesktopUpdateControl />);
    expect(container).toBeEmptyDOMElement();
  });

  it("renders the waiting update the platform reports", async () => {
    const user = userEvent.setup();
    harness.capability = {
      pending: async () => WAITING as PlatformPendingUpdate,
      install: async () => undefined,
      onAvailable: () => () => undefined,
    };
    render(<DesktopUpdateControl />);
    const control = await screen.findByRole("button", { name: "Update to Pi Dash 2.0.0" });
    await user.click(control);
    expect(screen.getByRole("alert")).toHaveTextContent(UPDATE_NO_RESTART_TEXT);
  });
});
