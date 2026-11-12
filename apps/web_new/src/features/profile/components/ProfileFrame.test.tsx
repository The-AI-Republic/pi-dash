// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Profile page frame (parity/specs/profile.md §1 and §4, PROF-001/002/008):
// the crumb labels the member, the role decides strip versus picker, and
// every tab renders through the outlet beside the identity panel.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { UserProfileSegregation } from "@pidash/api-client";

const state = vi.hoisted(() => ({ narrow: false }));
vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    Outlet: () => <div data-testid="frame-outlet">tab body</div>,
    Link: (props: { to: string; children?: React.ReactNode; "aria-current"?: "page"; "data-profile-tab"?: string }) => (
      <a href={props.to} aria-current={props["aria-current"]} data-profile-tab={props["data-profile-tab"]}>
        {props.children}
      </a>
    ),
    useRouterState: (options: { select: (value: { location: { pathname: string } }) => string }) =>
      options.select({ location: { pathname: "/acme/profile/u-1/assigned/" } }),
    useNavigate: () => vi.fn(),
  };
});
vi.mock("../../../shared/shell/narrowViewport.js", () => ({
  useNarrowViewport: () => state.narrow,
}));

import { ProfileFrame } from "./ProfileFrame.js";

afterEach(() => {
  state.narrow = false;
});

const segregation: UserProfileSegregation = {
  user_data: {
    email: "opal@example.com",
    first_name: "Opal",
    last_name: "Whelan",
    display_name: "Opal W.",
    avatar_url: null,
    cover_image_url: null,
    date_joined: "2026-03-09T12:00:00Z",
    user_timezone: "UTC",
  },
  project_data: [],
};

function renderFrame(args: { isOwn?: boolean } = {}) {
  return render(
    <ProfileFrame
      workspaceSlug="acme"
      userId="u-1"
      segregation={segregation}
      permissions={{ role: 20, isMember: true, canEdit: true, isAdmin: true }}
      isOwn={args.isOwn ?? false}
      projects={[]}
      apiBaseUrl="https://api.test"
    />
  );
}

describe("ProfileFrame", () => {
  it("names the member in the crumb and renders the strip, sidebar and outlet", () => {
    renderFrame();
    expect(screen.getByTestId("profile-page")).toBeInTheDocument();
    expect(screen.getByText("Opal Whelan Work")).toBeInTheDocument();
    expect(screen.getByTestId("profile-tabs")).toBeInTheDocument();
    expect(screen.getByTestId("profile-sidebar")).toBeInTheDocument();
    expect(screen.getByTestId("frame-outlet")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Assigned" })).toHaveAttribute("aria-current", "page");
  });

  it("reads Your work on your own profile", () => {
    renderFrame({ isOwn: true });
    expect(screen.getByText("Your work")).toBeInTheDocument();
  });

  it("swaps the strip for the narrow picker", () => {
    state.narrow = true;
    renderFrame();
    expect(screen.queryByTestId("profile-tabs")).toBeNull();
    expect(screen.getByTestId("profile-tab-picker")).toBeInTheDocument();
    expect(screen.getByTestId("profile-tab-picker-label")).toHaveTextContent("Assigned");
  });
});
