// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import {
  MAINTENANCE_ARM_DARK,
  MAINTENANCE_ARM_LIGHT,
  MAINTENANCE_HEADING,
  MAINTENANCE_SUPPORT_HREF,
  MaintenanceScreen,
  maintenanceIllustration,
} from "./MaintenanceScreen.js";

afterEach(() => {
  document.documentElement.removeAttribute("data-theme");
});

describe("maintenanceIllustration", () => {
  it("swaps artwork with the color scheme", () => {
    expect(maintenanceIllustration(false)).toBe(MAINTENANCE_ARM_LIGHT);
    expect(maintenanceIllustration(true)).toBe(MAINTENANCE_ARM_DARK);
  });
});

describe("MaintenanceScreen", () => {
  it("explains the startup failure with a support route and no navigation", () => {
    render(<MaintenanceScreen />);
    expect(screen.getByTestId("instance-maintenance")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: MAINTENANCE_HEADING })).toBeInTheDocument();
    expect(screen.getByText(/Check the container logs/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Contact Support" })).toHaveAttribute("href", MAINTENANCE_SUPPORT_HREF);
    expect(screen.queryAllByRole("navigation")).toHaveLength(0);
  });

  it("uses the light artwork in the light scheme", () => {
    render(<MaintenanceScreen />);
    expect(document.querySelector("[data-maintenance-illustration]")).toHaveAttribute("src", MAINTENANCE_ARM_LIGHT);
  });

  it("uses the dark artwork when the document scheme is dark", () => {
    document.documentElement.setAttribute("data-theme", "dark");
    render(<MaintenanceScreen />);
    expect(document.querySelector("[data-maintenance-illustration]")).toHaveAttribute("src", MAINTENANCE_ARM_DARK);
  });

  it("treats the high-contrast dark scheme as dark", () => {
    document.documentElement.setAttribute("data-theme", "dark-contrast");
    render(<MaintenanceScreen />);
    expect(document.querySelector("[data-maintenance-illustration]")).toHaveAttribute("src", MAINTENANCE_ARM_DARK);
  });

  it("treats custom and unknown schemes by their light default", () => {
    document.documentElement.setAttribute("data-theme", "custom");
    render(<MaintenanceScreen />);
    expect(document.querySelector("[data-maintenance-illustration]")).toHaveAttribute("src", MAINTENANCE_ARM_LIGHT);
  });
});
