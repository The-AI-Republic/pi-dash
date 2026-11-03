// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Tab outlet (parity/specs/profile.md §5, PROF-009/010): the registered
// body for the active tab, a quiet loading frame until a slice registers.
import { act, fireEvent, render, screen } from "@testing-library/react";
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

  it("never carries one tab's hook state into the next", () => {
    function Counter({ label }: { label: string }): React.ReactElement {
      const [count, setCount] = React.useState(0);
      return (
        <button type="button" onClick={() => setCount((value) => value + 1)}>
          {`${label} ${count}`}
        </button>
      );
    }
    act(() => {
      registerProfileSettingsPage("preferences", () => <Counter label="preferences" />);
      registerProfileSettingsPage("integrations", () => <Counter label="integrations" />);
    });
    const { rerender } = render(<SettingsTabOutlet tab="preferences" />);
    fireEvent.click(screen.getByRole("button", { name: "preferences 0" }));
    fireEvent.click(screen.getByRole("button", { name: "preferences 1" }));
    expect(screen.getByRole("button", { name: "preferences 2" })).toBeInTheDocument();

    rerender(<SettingsTabOutlet tab="integrations" />);
    expect(screen.getByRole("button", { name: "integrations 0" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "integrations 0" }));
    expect(screen.getByRole("button", { name: "integrations 1" })).toBeInTheDocument();

    // Back on preferences: a freshly mounted body starts at its own zero
    // instead of inheriting the count from the tab that was just shown
    // (the outlet used to call the page as a function, so both shared the
    // outlet's hook slots).
    rerender(<SettingsTabOutlet tab="preferences" />);
    expect(screen.getByRole("button", { name: "preferences 0" })).toBeInTheDocument();
  });
});

describe("instance-withheld tabs", () => {
  it("does not mount a cloud-withheld body even when it is registered", () => {
    registerProfileSettingsPage("security", () => <p>password body</p>);
    render(<SettingsTabOutlet tab="security" selfManaged={false} />);
    expect(screen.queryByText("password body")).not.toBeInTheDocument();
    expect(screen.getByTestId("profile-settings-tab-loading")).toHaveAttribute("aria-busy", "true");
  });

  it("mounts the body on a self-managed instance", () => {
    registerProfileSettingsPage("security", () => <p>password body</p>);
    render(<SettingsTabOutlet tab="security" selfManaged />);
    expect(screen.getByText("password body")).toBeInTheDocument();
  });
});
