// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Tab outlet (parity/specs/profile.md §5, PROF-009/010): the registered
// body for the active tab, a quiet loading frame until a slice registers.
import { act, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { registerProfileSettingsPage, resetProfileSettingsPages } from "../lib/registry.js";
import { SettingsTabOutlet } from "./SettingsTabOutlet.js";

afterEach(() => resetProfileSettingsPages());

describe("SettingsTabOutlet", () => {
  it("keeps the frame busy while the tab has no body", () => {
    render(<SettingsTabOutlet tab="preferences" />);
    expect(screen.getByTestId("profile-settings-tab-loading")).toHaveAttribute("aria-busy", "true");
  });

  it("mounts the registered body and follows a later registration", () => {
    render(<SettingsTabOutlet tab="preferences" />);
    expect(screen.queryByText("preferences body")).not.toBeInTheDocument();
    act(() => {
      registerProfileSettingsPage("preferences", () => <p>preferences body</p>);
    });
    expect(screen.getByText("preferences body")).toBeInTheDocument();
    expect(screen.queryByTestId("profile-settings-tab-loading")).not.toBeInTheDocument();
  });

  it("falls back to the loading frame when a body unregisters", () => {
    let unregister: () => void = () => undefined;
    act(() => {
      unregister = registerProfileSettingsPage("activity", () => <p>activity body</p>);
    });
    render(<SettingsTabOutlet tab="activity" />);
    expect(screen.getByText("activity body")).toBeInTheDocument();
    act(() => unregister());
    expect(screen.getByTestId("profile-settings-tab-loading")).toBeInTheDocument();
  });
});
