// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Not-authorized surface (WSET-002/007, spec §1/§2/§36): the heading
// names the refusal, no sidebar renders, the tab title stays empty and
// the way back is an ordinary link (no redirect).
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { SETTINGS_NOT_AUTHORIZED_HEADING, SettingsNotAuthorized } from "./SettingsNotAuthorized.js";

afterEach(() => {
  document.title = "";
  document.querySelector('meta[name="robots"]')?.remove();
});

describe("SettingsNotAuthorized", () => {
  it("explains the refusal and offers the way back", () => {
    render(<SettingsNotAuthorized workspaceSlug="acme" />);
    expect(screen.getByRole("heading", { name: SETTINGS_NOT_AUTHORIZED_HEADING })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Back to workspace" })).toHaveAttribute("href", "/acme");
  });

  it("renders no settings sidebar", () => {
    render(<SettingsNotAuthorized workspaceSlug="acme" />);
    expect(screen.queryByTestId("workspace-settings-sidebar")).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Members" })).not.toBeInTheDocument();
  });

  it("leaves the tab title empty", () => {
    document.title = "Previous title";
    render(<SettingsNotAuthorized workspaceSlug="acme" />);
    expect(document.title).toBe("");
    expect(document.querySelector('meta[name="robots"]')).toHaveAttribute("content", "noindex, nofollow");
  });
});
