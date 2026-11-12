// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Attribution rows (parity/specs/pages.md §6.9, §13.3, PAGE-050): the last
// editor with a relative age and the creator with a calendar date, each a
// profile link, and the deactivated fallback that keeps the row intact.

import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  Link: ({ to, children, ...rest }: { to: string; children: React.ReactNode }) => (
    <a href={to} {...rest}>
      {children}
    </a>
  ),
}));

import { PageActorsInfo, PAGE_CREATED_BY_LABEL, PAGE_EDITED_BY_LABEL } from "./PageActorsInfo.js";
import { PAGE_DEACTIVATED_USER, type PageActorRow } from "../lib/pagePane.js";

const ROWS: PageActorRow[] = [
  {
    role: "edited",
    userId: "u-ada",
    name: "Ada Byron",
    deactivated: false,
    avatarUrl: "/api/assets/ada/",
    timeText: "3h ago",
    href: "/acme/profile/u-ada/",
  },
  {
    role: "created",
    userId: "u-ada",
    name: "Ada Byron",
    deactivated: false,
    avatarUrl: null,
    timeText: "Sep 1, 2026",
    href: "/acme/profile/u-ada/",
  },
];

function rowOf(label: string): HTMLElement {
  const sibling = screen.getByText(label).nextElementSibling;
  if (sibling === null) throw new Error(`no row after ${label}`);
  return sibling as HTMLElement;
}

describe("PageActorsInfo", () => {
  it("links both names to the profile with their times", () => {
    render(<PageActorsInfo rows={ROWS} />);
    expect(screen.getByText(PAGE_EDITED_BY_LABEL)).toBeInTheDocument();
    const edited = rowOf(PAGE_EDITED_BY_LABEL);
    const link = Array.from(edited.querySelectorAll("a"))[0];
    expect(link?.getAttribute("href")).toBe("/acme/profile/u-ada/");
    expect(link?.textContent).toBe("Ada Byron");
    expect(edited.querySelector("span")?.textContent).toBe("3h ago");
    expect(rowOf(PAGE_CREATED_BY_LABEL).querySelector("span")?.textContent).toBe("Sep 1, 2026");
  });

  it("paints an avatar for a member and initials when the directory has none", () => {
    render(<PageActorsInfo rows={ROWS} />);
    expect(rowOf(PAGE_EDITED_BY_LABEL).querySelector("img")?.getAttribute("src")).toBe("/api/assets/ada/");
    expect(rowOf(PAGE_CREATED_BY_LABEL).querySelector("img")).toBeNull();
  });

  it("falls back to the deactivated label, keeping the row and its address", () => {
    render(
      <PageActorsInfo
        rows={[
          { ...ROWS[0]!, name: PAGE_DEACTIVATED_USER, deactivated: true, avatarUrl: "/api/assets/gone/" },
          ROWS[1]!,
        ]}
      />
    );
    const edited = rowOf(PAGE_EDITED_BY_LABEL);
    expect(edited.textContent).toContain(PAGE_DEACTIVATED_USER);
    expect(edited.querySelector("a")?.getAttribute("href")).toBe("/acme/profile/u-ada/");
    expect(edited.querySelector("img")).toBeNull();
    expect(edited.querySelector("span")?.textContent).toBe("3h ago");
  });

  it("renders a name without a link when there is nobody to link", () => {
    render(
      <PageActorsInfo
        rows={[
          { ...ROWS[0]!, userId: null, name: PAGE_DEACTIVATED_USER, deactivated: true, href: null },
          { ...ROWS[1]!, userId: null, href: null },
        ]}
      />
    );
    expect(rowOf(PAGE_EDITED_BY_LABEL).querySelector("a")).toBeNull();
    expect(rowOf(PAGE_EDITED_BY_LABEL).textContent).toContain(PAGE_DEACTIVATED_USER);
    expect(rowOf(PAGE_CREATED_BY_LABEL).textContent).toContain("Ada Byron");
  });
});
