// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Tab strip, narrow picker and inline denial (parity/specs/profile.md §1,
// PROF-002). The strip links each role tab and marks the current one; the
// picker offers exactly the role's tabs and labels the current tab (the
// NEWFRONT-433 target); the denial is the guest's issue-tab notice.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

const navigate = vi.fn();
vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return {
    ...original,
    Link: (props: { to: string; children?: React.ReactNode; "aria-current"?: "page"; "data-profile-tab"?: string }) => (
      <a href={props.to} aria-current={props["aria-current"]} data-profile-tab={props["data-profile-tab"]}>
        {props.children}
      </a>
    ),
    useNavigate: () => navigate,
  };
});

import { PROFILE_DENIED_DETAIL, PROFILE_DENIED_TITLE, ProfileAccessDenied } from "./ProfileAccessDenied.js";
import { ProfileMobileTabPicker, profilePickerLabel } from "./ProfileMobileTabPicker.js";
import { ProfileTabStrip } from "./ProfileTabStrip.js";

const member = { canEdit: true };
const guest = { canEdit: false };
const SUMMARY = ["Summary", "Assigned", "Created", "Subscribed", "Activity"];

describe("ProfileTabStrip", () => {
  it("links every tab for a member with trailing-slash addresses", () => {
    render(<ProfileTabStrip workspaceSlug="acme" userId="u-1" permissions={member} pathname="/acme/profile/u-1/" />);
    const strip = screen.getByTestId("profile-tabs");
    expect(
      within(strip)
        .getAllByRole("link")
        .map((link) => link.textContent)
    ).toEqual(SUMMARY);
    expect(within(strip).getByRole("link", { name: "Assigned" })).toHaveAttribute(
      "href",
      "/acme/profile/u-1/assigned/"
    );
    expect(within(strip).getByRole("link", { name: "Summary" })).toHaveAttribute("href", "/acme/profile/u-1/");
  });

  it("marks the current tab and clears the mark on another", () => {
    const { rerender } = render(
      <ProfileTabStrip workspaceSlug="acme" userId="u-1" permissions={member} pathname="/acme/profile/u-1/created" />
    );
    expect(screen.getByRole("link", { name: "Created" })).toHaveAttribute("aria-current", "page");
    rerender(
      <ProfileTabStrip workspaceSlug="acme" userId="u-1" permissions={member} pathname="/acme/profile/u-1/assigned/" />
    );
    expect(screen.getByRole("link", { name: "Assigned" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("link", { name: "Created" })).not.toHaveAttribute("aria-current");
  });

  it("gives guests the summary tab alone", () => {
    render(<ProfileTabStrip workspaceSlug="acme" userId="u-1" permissions={guest} pathname="/acme/profile/u-1/" />);
    expect(screen.getAllByRole("link").map((link) => link.textContent)).toEqual(["Summary"]);
  });

  it("marks no tab when the pathname names an unknown segment", () => {
    render(
      <ProfileTabStrip workspaceSlug="acme" userId="u-1" permissions={member} pathname="/acme/profile/u-1/bogus" />
    );
    expect(screen.getAllByRole("link").every((link) => !link.hasAttribute("aria-current"))).toBe(true);
  });
});

describe("ProfileMobileTabPicker", () => {
  it("labels the current tab on every address", () => {
    expect(profilePickerLabel("/acme/profile/u-1/", "acme", "u-1")).toBe("Summary");
    expect(profilePickerLabel("/acme/profile/u-1/assigned/", "acme", "u-1")).toBe("Assigned");
    expect(profilePickerLabel("/acme/profile/u-1/created", "acme", "u-1")).toBe("Created");
    expect(profilePickerLabel("/acme/profile/u-1/subscribed/", "acme", "u-1")).toBe("Subscribed");
    expect(profilePickerLabel("/acme/profile/u-1/activity", "acme", "u-1")).toBe("Activity");
    expect(profilePickerLabel("/acme/profile/u-1/bogus", "acme", "u-1")).toBe("Summary");
  });

  it("names the member's tabs, labels the current tab and navigates the chosen one", async () => {
    navigate.mockClear();
    render(
      <ProfileMobileTabPicker
        workspaceSlug="acme"
        userId="u-1"
        permissions={member}
        pathname="/acme/profile/u-1/created"
      />
    );
    expect(screen.getByTestId("profile-tab-picker-label")).toHaveTextContent("Created");
    expect(screen.getByRole("button", { name: "Profile tab, current Created" })).toBeInTheDocument();
    await userEvent.click(screen.getByTestId("profile-tab-picker-trigger"));
    const items = await screen.findAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual(SUMMARY);
    await userEvent.click(items[4]!);
    expect(navigate).toHaveBeenCalledWith({ href: "/acme/profile/u-1/activity/" });
  });

  it("offers only the summary to a guest", async () => {
    render(
      <ProfileMobileTabPicker workspaceSlug="acme" userId="u-1" permissions={guest} pathname="/acme/profile/u-1/" />
    );
    await userEvent.click(screen.getByTestId("profile-tab-picker-trigger"));
    const items = await screen.findAllByRole("menuitem");
    expect(items.map((item) => item.textContent)).toEqual(["Summary"]);
  });
});

describe("ProfileAccessDenied", () => {
  it("renders the inline denial copy", () => {
    render(<ProfileAccessDenied />);
    expect(screen.getByRole("heading", { name: PROFILE_DENIED_TITLE })).toBeInTheDocument();
    expect(screen.getByText(PROFILE_DENIED_DETAIL)).toBeInTheDocument();
  });
});
