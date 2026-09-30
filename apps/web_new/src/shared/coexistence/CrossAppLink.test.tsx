// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

// Stand-in for the router Link: the real one's navigation is TanStack's
// own tested behavior. What this suite owns is the branch decision —
// router link vs plain anchor — and prop passthrough.
vi.mock("@tanstack/react-router", () => ({
  // Plain anchor with a marker: the real Link's navigation is TanStack's
  // own tested behavior. href keeps the testing-library link role.
  Link: (props: { to?: string; children?: React.ReactNode }) => (
    <a data-router-link="" data-to={props.to} href={props.to}>
      {props.children}
    </a>
  ),
}));

import { CrossAppLink } from "./CrossAppLink.js";

describe("CrossAppLink", () => {
  it("renders an unmigrated screen as a plain anchor, with no router required", () => {
    // No RouterProvider here on purpose: old-app links must work anywhere.
    render(
      <CrossAppLink to="/acme/projects/1/issues" className="nav" target="_self">
        Issues
      </CrossAppLink>
    );
    const link = screen.getByRole("link", { name: "Issues" });
    expect(link.tagName).toBe("A");
    expect(link).toHaveAttribute("href", "/acme/projects/1/issues");
    expect(link).toHaveAttribute("class", "nav");
    expect(link).not.toHaveAttribute("data-router-link");
  });

  it("renders a migrated screen as a router link", () => {
    render(
      <CrossAppLink to="/sign-in" prefixes={["/sign-in"]}>
        Sign in
      </CrossAppLink>
    );
    const link = screen.getByRole("link", { name: "Sign in" });
    expect(link).toHaveAttribute("data-router-link");
    expect(link).toHaveAttribute("data-to", "/sign-in");
  });

  it("flips a link when its prefix joins the list", () => {
    const { rerender } = render(<CrossAppLink to="/sign-in">Sign in</CrossAppLink>);
    expect(screen.getByRole("link", { name: "Sign in" })).not.toHaveAttribute("data-router-link");
    rerender(
      <CrossAppLink to="/sign-in" prefixes={["/sign-in"]}>
        Sign in
      </CrossAppLink>
    );
    expect(screen.getByRole("link", { name: "Sign in" })).toHaveAttribute("data-router-link");
  });
});
