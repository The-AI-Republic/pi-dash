/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { IWorkspaceSidebarNavigationItem } from "@pi-dash/constants";

vi.mock("mobx-react", () => ({
  observer: (component: unknown) => component,
}));

// Renders the label the real SidebarItem would render, without pulling in the
// router/permission/preference stack it depends on.
vi.mock("@/pi-dash-web/components/workspace/sidebar/sidebar-item", () => ({
  SidebarItem: ({ item }: { item: IWorkspaceSidebarNavigationItem }) => <div>{item.labelTranslationKey}</div>,
}));

const { SidebarMenuItems } = await import("@/components/workspace/sidebar/sidebar-menu-items");

describe("workspace sidebar navigation rows", () => {
  it('no longer renders "Your work" — it moved to the user menu popup', () => {
    render(<SidebarMenuItems />);

    expect(screen.queryByText("Your work")).not.toBeInTheDocument();
  });

  it("still renders the built-in rows", () => {
    render(<SidebarMenuItems />);

    expect(screen.getByText("Home")).toBeInTheDocument();
    expect(screen.getByText("Pi Dash AI")).toBeInTheDocument();
  });
});
