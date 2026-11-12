// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import {
  getProfileSettingsPage,
  registerProfileSettingsPage,
  resetProfileSettingsPages,
  useProfileSettingsPage,
} from "./registry.js";

function Probe({ tab }: { tab: "general" | "security" }): React.ReactElement {
  const Page = useProfileSettingsPage(tab);
  return <div data-testid="probe">{Page === null ? "empty" : Page()}</div>;
}

afterEach(() => {
  resetProfileSettingsPages();
});

describe("profile settings page registry", () => {
  it("starts empty and throws on an unregistered tab id", () => {
    expect(getProfileSettingsPage("general")).toBeNull();
    expect(() => registerProfileSettingsPage("bogus" as never, () => null)).toThrow(
      /Unknown profile settings tab: bogus/
    );
  });

  it("renders a registered page and follows later registrations", () => {
    render(<Probe tab="general" />);
    expect(screen.getByTestId("probe")).toHaveTextContent("empty");
    act(() => {
      registerProfileSettingsPage("general", () => <span>general body</span>);
    });
    expect(screen.getByTestId("probe")).toHaveTextContent("general body");
    expect(getProfileSettingsPage("security")).toBeNull();
  });

  it("unregisters only its own entry", () => {
    const unregisterFirst = registerProfileSettingsPage("general", () => "first");
    const unregisterSecond = registerProfileSettingsPage("general", () => "second");
    // Unmounting the superseded registration leaves the newer one in place.
    unregisterFirst();
    expect(getProfileSettingsPage("general")?.()).toBe("second");
    unregisterSecond();
    expect(getProfileSettingsPage("general")).toBeNull();
  });
});
