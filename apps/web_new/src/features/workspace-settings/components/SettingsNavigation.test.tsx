// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Grouped navigation (WSET-001, spec §1): groups and entries in catalog
// order, one active entry, plain clicks navigate in-app, modified clicks
// keep their browser meaning.
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ROLE_ADMIN, ROLE_MEMBER, selectPermissions } from "../../../core/session/index.js";
import { visibleWorkspaceSettingsGroups } from "../lib/sections.js";
import { SettingsNavigation } from "./SettingsNavigation.js";

describe("SettingsNavigation", () => {
  it("lists groups and entries in catalog order with the current one active", () => {
    render(
      <SettingsNavigation
        idPrefix="group"
        groups={visibleWorkspaceSettingsGroups("acme", selectPermissions(ROLE_ADMIN))}
        activeSection="webhooks"
        onNavigate={vi.fn()}
      />
    );
    expect(screen.getByRole("heading", { name: "Administration" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Developer" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Webhooks" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("link", { name: "Members" })).not.toHaveAttribute("aria-current");
    expect(screen.getByRole("link", { name: "Members" })).toHaveAttribute("href", "/acme/settings/members/");
  });

  it("hides the Developer group from members", () => {
    render(
      <SettingsNavigation
        idPrefix="group"
        groups={visibleWorkspaceSettingsGroups("acme", selectPermissions(ROLE_MEMBER))}
        activeSection="general"
        onNavigate={vi.fn()}
      />
    );
    expect(screen.getByRole("heading", { name: "Administration" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Developer" })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Billing & Plans" })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Webhooks" })).not.toBeInTheDocument();
  });

  it("navigates on a plain click and closes its surface through onFollow", () => {
    const onNavigate = vi.fn();
    const onFollow = vi.fn();
    render(
      <SettingsNavigation
        idPrefix="group"
        groups={visibleWorkspaceSettingsGroups("acme", selectPermissions(ROLE_ADMIN))}
        activeSection="general"
        onNavigate={onNavigate}
        onFollow={onFollow}
      />
    );
    fireEvent.click(screen.getByRole("link", { name: "Exports" }));
    expect(onFollow).toHaveBeenCalledOnce();
    expect(onNavigate).toHaveBeenCalledWith("/acme/settings/exports/");
  });

  it("leaves modified clicks to the browser", () => {
    const onNavigate = vi.fn();
    render(
      <SettingsNavigation
        idPrefix="group"
        groups={visibleWorkspaceSettingsGroups("acme", selectPermissions(ROLE_ADMIN))}
        activeSection="general"
        onNavigate={onNavigate}
      />
    );
    for (const modifier of [{ metaKey: true }, { ctrlKey: true }, { shiftKey: true }, { altKey: true }]) {
      fireEvent.click(screen.getByRole("link", { name: "Members" }), modifier);
    }
    expect(onNavigate).not.toHaveBeenCalled();
  });
});
