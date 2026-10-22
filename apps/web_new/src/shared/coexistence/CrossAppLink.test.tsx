// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

// Stand-in for the router Link: the real one's navigation is TanStack's
// own tested behavior. What this suite owns is the branch decision —
// router link vs plain anchor — and prop passthrough.
vi.mock("@tanstack/react-router", () => ({
  // Plain anchor with a marker: the real Link's navigation is TanStack's
  // own tested behavior. href keeps the testing-library link role.
  Link: (props: { to?: string; hash?: string; children?: React.ReactNode }) => (
    <a data-router-link="" data-to={props.to} data-hash={props.hash} href={props.to}>
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
    const { rerender } = render(<CrossAppLink to="/acme/projects/1/issues">Issues</CrossAppLink>);
    expect(screen.getByRole("link", { name: "Issues" })).not.toHaveAttribute("data-router-link");
    rerender(
      <CrossAppLink to="/acme/projects/1/issues" prefixes={["/acme"]}>
        Issues
      </CrossAppLink>
    );
    expect(screen.getByRole("link", { name: "Issues" })).toHaveAttribute("data-router-link");
  });

  it("appends a deep-link anchor on a plain anchor", () => {
    // Empty prefixes pin the anchor branch: the bundled list migrates
    // /settings/profile (NEWFRONT-586), which would flip this to a link.
    render(
      <CrossAppLink to="/settings/profile/ai-assistant" hash="voice-dictation" prefixes={[]}>
        Voice
      </CrossAppLink>
    );
    expect(screen.getByRole("link", { name: "Voice" })).toHaveAttribute(
      "href",
      "/settings/profile/ai-assistant#voice-dictation"
    );
  });

  it("hands a deep-link anchor to the router link", () => {
    render(
      <CrossAppLink to="/settings/profile/ai-assistant" hash="voice-dictation" prefixes={["/settings"]}>
        Voice
      </CrossAppLink>
    );
    const link = screen.getByRole("link", { name: "Voice" });
    expect(link).toHaveAttribute("data-router-link");
    expect(link).toHaveAttribute("data-hash", "voice-dictation");
  });

  it("forwards a ref to the anchor", () => {
    const seen: Array<HTMLAnchorElement | null> = [];
    render(
      <CrossAppLink
        to="/acme/projects/1/issues"
        ref={(element) => {
          seen.push(element);
        }}
      >
        Issues
      </CrossAppLink>
    );
    expect(seen).toHaveLength(1);
    expect(seen[0]?.tagName).toBe("A");
    expect(seen[0]).toHaveAttribute("href", "/acme/projects/1/issues");
  });
});
