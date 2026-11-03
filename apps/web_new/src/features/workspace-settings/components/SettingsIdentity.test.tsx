// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sidebar identity (WSET-001, spec §1): back control, logo or initial
// fallback, workspace name and role.
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ROLE_ADMIN, ROLE_MEMBER } from "../../../core/session/index.js";
import { SettingsIdentity } from "./SettingsIdentity.js";

function renderIdentity(overrides: Partial<React.ComponentProps<typeof SettingsIdentity>> = {}) {
  const onBack = vi.fn();
  render(
    <SettingsIdentity
      workspaceSlug="acme"
      workspaceName="Acme"
      logoUrl={null}
      role={ROLE_ADMIN}
      onBack={onBack}
      {...overrides}
    />
  );
  return { onBack };
}

describe("SettingsIdentity", () => {
  it("names the workspace, its role and the way back", () => {
    renderIdentity();
    expect(screen.getByText("Acme")).toBeInTheDocument();
    expect(screen.getByText("Admin")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /Back to workspace/ })).toHaveAttribute("href", "/acme");
    expect(screen.getByTestId("workspace-logo")).toHaveTextContent("A");
  });

  it("shows a member role and an uploaded logo", () => {
    renderIdentity({ role: ROLE_MEMBER, logoUrl: "https://cdn.test/logo.png" });
    expect(screen.getByText("Member")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Workspace logo" })).toBeInTheDocument();
  });

  it("navigates back on a plain click", () => {
    const { onBack } = renderIdentity();
    fireEvent.click(screen.getByRole("link", { name: /Back to workspace/ }));
    expect(onBack).toHaveBeenCalledOnce();
  });

  it("leaves modified clicks to the browser", () => {
    const { onBack } = renderIdentity();
    for (const modifier of [{ metaKey: true }, { ctrlKey: true }, { shiftKey: true }, { altKey: true }]) {
      fireEvent.click(screen.getByRole("link", { name: /Back to workspace/ }), modifier);
    }
    expect(onBack).not.toHaveBeenCalled();
  });
});
