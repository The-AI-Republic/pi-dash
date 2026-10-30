// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Identity side panel (parity/specs/profile.md §4, PROF-008). Target
// behavior: no cover art, avatar photo or initial, both names, join date,
// timezone clock, one expandable project row with counts, the own-only
// pencil navigating to settings, and collapse plus the narrow overlay.
import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { UserProfileSegregation } from "@pidash/api-client";

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  // Anchor stand-in (CrossAppLink.test.tsx precedent): the pencil targets
  // a migrated prefix, so it renders as a router link.
  return {
    ...original,
    Link: (props: { to?: string; children?: React.ReactNode } & Record<string, unknown>) => {
      const { to, children, ...rest } = props;
      return (
        <a data-router-link="" data-to={to} href={to} {...(rest as React.AnchorHTMLAttributes<HTMLAnchorElement>)}>
          {children as React.ReactNode}
        </a>
      );
    },
  };
});

import {
  PROFILE_CLOCK_TICK_MS,
  profileSettingsGeneralHref,
  ProfileSidebarFrame,
  useProfileClock,
} from "./ProfileSidebar.js";

function segregation(overrides: Partial<UserProfileSegregation["user_data"]> = {}): UserProfileSegregation {
  return {
    user_data: {
      email: "opal@example.com",
      first_name: "Opal",
      last_name: "Whelan",
      display_name: "Opal W.",
      avatar_url: null,
      cover_image_url: "https://files.example.test/cover.png",
      date_joined: "2026-03-09T12:00:00Z",
      user_timezone: "Pacific/Kiritimati",
      ...overrides,
    },
    project_data: [
      { id: "p-1", logo_props: {}, created_issues: 3, assigned_issues: 4, completed_issues: 1, pending_issues: 2 },
      { id: "p-2", logo_props: {}, created_issues: 0, assigned_issues: 0, completed_issues: 0, pending_issues: 0 },
    ],
  };
}

function renderFrame(args: { isOwn?: boolean; narrow?: boolean; avatar?: string | null } = {}) {
  return render(
    <ProfileSidebarFrame
      segregation={segregation(args.avatar === undefined ? {} : { avatar_url: args.avatar })}
      workspaceSlug="acme"
      isOwn={args.isOwn ?? true}
      projects={[
        { id: "p-1", name: "Prof Proj" },
        { id: "p-2", name: "Quiet Proj" },
      ]}
      apiBaseUrl="https://api.test"
      narrow={args.narrow ?? false}
    />
  );
}

afterEach(() => {
  vi.useRealTimers();
});

describe("ProfileSidebarFrame", () => {
  it("shows the identity without any cover art", () => {
    renderFrame();
    const panel = screen.getByTestId("profile-sidebar");
    expect(within(panel).getByText("Opal Whelan")).toBeInTheDocument();
    expect(within(panel).getByText("(Opal W.)")).toBeInTheDocument();
    expect(screen.queryByRole("img", { name: /cover/i })).toBeNull();
    expect(panel.innerHTML).not.toContain("cover.png");
  });

  it("falls back to an uppercase initial and renders a set photo as an image", () => {
    const { unmount, container } = renderFrame();
    expect(screen.getByRole("img", { name: "Opal Whelan" })).toHaveTextContent("O");
    expect(container.querySelector('[data-profile-avatar="initial"]')).not.toBeNull();
    unmount();
    const photo = renderFrame({ avatar: "/files/avatar/u-1.png" });
    const frame = screen.getByRole("img", { name: "Opal Whelan" });
    expect(frame.querySelector("img")?.getAttribute("src")).toBe("https://api.test/files/avatar/u-1.png");
    const marker = photo.container.querySelector('[data-profile-avatar="image"]');
    expect(marker?.getAttribute("data-profile-avatar-src")).toBe("https://api.test/files/avatar/u-1.png");
  });

  it("hides a blank display name", () => {
    render(
      <ProfileSidebarFrame
        segregation={segregation({ display_name: "  " })}
        workspaceSlug="acme"
        isOwn={false}
        projects={[]}
        apiBaseUrl="https://api.test"
        narrow={false}
      />
    );
    expect(screen.queryByText(/^\(.*\)$/)).toBeNull();
  });

  it("shows the join date and the member's timezone clock", () => {
    renderFrame();
    expect(screen.getByText("Mar 9, 2026")).toBeInTheDocument();
    expect(screen.getByText(/^\d{2}:\d{2} Pacific\/Kiritimati$/)).toBeInTheDocument();
  });

  it("renders one project row with a completion tint and expandable server counts", async () => {
    renderFrame();
    const rows = document.querySelectorAll("[data-profile-project-row]");
    expect(rows).toHaveLength(2);
    expect(screen.getByText("Prof Proj")).toBeInTheDocument();
    expect(screen.getByText("Quiet Proj")).toBeInTheDocument();
    expect(screen.getByText("25%")).toBeInTheDocument();
    expect(screen.getByText("0%")).toBeInTheDocument();

    const button = screen.getByRole("button", { name: "Prof Proj, 25% complete" });
    expect(button).toHaveAttribute("aria-expanded", "false");
    await userEvent.click(button);
    expect(button).toHaveAttribute("aria-expanded", "true");
    const counts = document.querySelector('[data-profile-project-counts="p-1"]') as HTMLElement;
    expect(within(counts).getByText("Created").nextElementSibling).toHaveTextContent("3");
    expect(within(counts).getByText("Assigned").nextElementSibling).toHaveTextContent("4");
    expect(within(counts).getByText("Due").nextElementSibling).toHaveTextContent("2");
    expect(within(counts).getByText("Completed").nextElementSibling).toHaveTextContent("1");
    await userEvent.click(button);
    expect(document.querySelector('[data-profile-project-counts="p-1"]')).toBeNull();
  });

  it("shows a neutral placeholder when the workspace row is missing", () => {
    render(
      <ProfileSidebarFrame
        segregation={segregation()}
        workspaceSlug="acme"
        isOwn={false}
        projects={[]}
        apiBaseUrl="https://api.test"
        narrow={false}
      />
    );
    expect(screen.getAllByText("Unavailable project")).toHaveLength(2);
    expect(screen.queryByText("p-1")).toBeNull();
    expect(screen.queryByText("p-2")).toBeNull();
  });

  it("offers the edit pencil only on your own profile, linking to the settings general tab", () => {
    const { unmount } = renderFrame({ isOwn: false });
    expect(screen.queryByTestId("profile-edit-pencil")).toBeNull();
    unmount();
    renderFrame({ isOwn: true });
    const pencil = screen.getByTestId("profile-edit-pencil");
    expect(pencil).toHaveAttribute("href", profileSettingsGeneralHref("acme"));
    expect(pencil).toHaveAttribute("href", "/acme/settings/account/general/");
  });

  it("collapses on demand and reopens from the show control", async () => {
    renderFrame();
    await userEvent.click(screen.getByRole("button", { name: "Hide profile details" }));
    expect(screen.queryByTestId("profile-sidebar")).toBeNull();
    await userEvent.click(screen.getByTestId("profile-sidebar-toggle"));
    expect(screen.getByTestId("profile-sidebar")).toBeInTheDocument();
  });

  it("starts collapsed on narrow screens and toggles as an overlay with a backdrop", async () => {
    renderFrame({ narrow: true });
    expect(screen.queryByTestId("profile-sidebar")).toBeNull();
    await userEvent.click(screen.getByTestId("profile-sidebar-toggle"));
    expect(screen.getByTestId("profile-sidebar")).toBeInTheDocument();
    expect(screen.getByTestId("profile-sidebar-backdrop")).toBeInTheDocument();
    await userEvent.click(screen.getByTestId("profile-sidebar-backdrop"));
    expect(screen.queryByTestId("profile-sidebar")).toBeNull();
  });

  it("follows a viewport change from wide to narrow", () => {
    const { rerender } = render(
      <ProfileSidebarFrame
        segregation={segregation()}
        workspaceSlug="acme"
        isOwn={true}
        projects={[]}
        apiBaseUrl="https://api.test"
        narrow={false}
      />
    );
    expect(screen.getByTestId("profile-sidebar")).toBeInTheDocument();
    rerender(
      <ProfileSidebarFrame
        segregation={segregation()}
        workspaceSlug="acme"
        isOwn={true}
        projects={[]}
        apiBaseUrl="https://api.test"
        narrow={true}
      />
    );
    expect(screen.queryByTestId("profile-sidebar")).toBeNull();
  });
});

describe("useProfileClock", () => {
  it("re-reads the wall time on the tick interval and clears it on unmount", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-03-09T12:34:00Z"));
    const readings: string[] = [];
    function Probe(): React.ReactElement {
      readings.push(useProfileClock("UTC"));
      return <span />;
    }
    const { unmount } = render(<Probe />);
    const first = readings[readings.length - 1];
    expect(first).toMatch(/^\d{2}:\d{2} UTC$/);
    act(() => {
      vi.advanceTimersByTime(PROFILE_CLOCK_TICK_MS);
    });
    expect(readings[readings.length - 1]).not.toBe(first);
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });
});
