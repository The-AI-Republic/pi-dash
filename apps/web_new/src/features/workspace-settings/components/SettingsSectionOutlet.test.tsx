// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Section outlet (WSET-001/036): the route renders whatever the section
// slice registered, and nothing at all until then.
import { act, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import { registerWorkspaceSettingsPage, resetWorkspaceSettingsPages } from "../lib/registry.js";
import { SettingsSectionOutlet } from "./SettingsSectionOutlet.js";

afterEach(() => {
  resetWorkspaceSettingsPages();
});

describe("SettingsSectionOutlet", () => {
  it("renders the honest placeholder before its slice registers", () => {
    render(<SettingsSectionOutlet workspaceSlug="acme" section="members" />);
    expect(screen.getByTestId("workspace-settings-section-placeholder")).toBeInTheDocument();
  });

  it("renders the registered body with the workspace slug", async () => {
    let unregister = (): void => undefined;
    await act(async () => {
      unregister = registerWorkspaceSettingsPage("members", ({ workspaceSlug }) => <p>roster of {workspaceSlug}</p>);
    });
    render(<SettingsSectionOutlet workspaceSlug="acme" section="members" />);
    expect(screen.getByText("roster of acme")).toBeInTheDocument();
    await act(async () => {
      unregister();
    });
    expect(screen.queryByText("roster of acme")).not.toBeInTheDocument();
    expect(screen.getByTestId("workspace-settings-section-placeholder")).toBeInTheDocument();
  });
});
