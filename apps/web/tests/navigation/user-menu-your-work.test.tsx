/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, screen, fireEvent } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { EUserWorkspaceRoles } from "@pi-dash/types";

const { push, allowPermissions, currentUser } = vi.hoisted(() => ({
  push: vi.fn(),
  allowPermissions: vi.fn(),
  currentUser: { id: "user-1", display_name: "Ada", email: "ada@example.com" } as Record<string, unknown>,
}));

vi.mock("mobx-react", () => ({
  observer: (component: unknown) => component,
}));

vi.mock("next/navigation", () => ({
  useRouter: () => ({ push }),
  useParams: () => ({ workspaceSlug: "acme" }),
}));

vi.mock("@pi-dash/i18n", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@pi-dash/propel/toast", () => ({
  TOAST_TYPE: { ERROR: "error" },
  setToast: vi.fn(),
}));

vi.mock("@pi-dash/propel/icons", () => ({
  YourWorkIcon: (props: { className?: string }) => <span data-testid="your-work-icon" {...props} />,
}));

// Stands in for the headless-ui-backed dropdown: renders the menu body inline
// and each item as the <button> the real MenuItem renders.
const CustomMenuMock = ({ children }: { children: React.ReactNode }) => <div>{children}</div>;
CustomMenuMock.MenuItem = ({ children, onClick }: { children: React.ReactNode; onClick?: () => void }) => (
  <button type="button" onClick={onClick}>
    {children}
  </button>
);
const AvatarMock = () => <span />;

vi.mock("@pi-dash/ui", () => ({
  CustomMenu: CustomMenuMock,
  Avatar: AvatarMock,
}));

vi.mock("@/components/common/cover-image", () => ({
  CoverImage: () => <span />,
}));

vi.mock("@/components/sidebar/sidebar-item", () => ({
  AppSidebarItem: () => <span />,
}));

vi.mock("@/hooks/store/use-app-theme", () => ({
  useAppTheme: () => ({ toggleAnySidebarDropdown: vi.fn() }),
}));

vi.mock("@/hooks/store/use-command-palette", () => ({
  useCommandPalette: () => ({ toggleProfileSettingsModal: vi.fn() }),
}));

vi.mock("@/hooks/store/user", () => ({
  useUser: () => ({ data: currentUser, signOut: vi.fn() }),
  useUserPermissions: () => ({ allowPermissions }),
}));

vi.mock("@/pi-dash-web/components/license", () => ({
  PaidPlanUpgradeModal: () => <span />,
}));

const { UserMenuRoot } = await import("@/components/workspace/sidebar/user-menu-root");

describe('user menu popup — "Your work"', () => {
  beforeEach(() => {
    push.mockClear();
    allowPermissions.mockReset();
  });

  it.each(["sidebar", "compact"] as const)(
    "renders the entry with its icon in the %s variant and navigates to the current user's profile",
    (variant) => {
      allowPermissions.mockReturnValue(true);

      render(<UserMenuRoot variant={variant} />);

      const entry = screen.getByRole("button", { name: /Your work/ });
      expect(entry).toBeInTheDocument();
      expect(screen.getByTestId("your-work-icon")).toBeInTheDocument();

      fireEvent.click(entry);
      expect(push).toHaveBeenCalledWith("/acme/profile/user-1");
    }
  );

  it("is placed above Settings", () => {
    allowPermissions.mockReturnValue(true);

    render(<UserMenuRoot variant="sidebar" />);

    const labels = screen.getAllByRole("button").map((button) => button.textContent);
    expect(labels.indexOf("Your work")).toBeLessThan(labels.indexOf("Settings"));
  });

  it("checks workspace-level member permissions, the same access list the sidebar row used", () => {
    allowPermissions.mockReturnValue(true);

    render(<UserMenuRoot variant="sidebar" />);

    expect(allowPermissions).toHaveBeenCalledWith(
      [EUserWorkspaceRoles.ADMIN, EUserWorkspaceRoles.MEMBER],
      "WORKSPACE",
      "acme"
    );
  });

  it("is hidden when the workspace role is not allowed (guests)", () => {
    allowPermissions.mockReturnValue(false);

    render(<UserMenuRoot variant="sidebar" />);

    expect(screen.queryByRole("button", { name: /Your work/ })).not.toBeInTheDocument();
    // the rest of the menu is unaffected
    expect(screen.getByRole("button", { name: /Settings/ })).toBeInTheDocument();
  });
});
