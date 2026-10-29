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
    // Cycles stand in for the unmigrated screen: project issues moved with
    // the Issues area gate (NEWFRONT-658).
    render(
      <CrossAppLink to="/acme/projects/1/cycles" className="nav" target="_self">
        Cycles
      </CrossAppLink>
    );
    const link = screen.getByRole("link", { name: "Cycles" });
    expect(link.tagName).toBe("A");
    expect(link).toHaveAttribute("href", "/acme/projects/1/cycles");
    expect(link).toHaveAttribute("class", "nav");
    expect(link).not.toHaveAttribute("data-router-link");
  });

  it("renders a reserved top-level area as a plain anchor even when a workspace matches", () => {
    render(<CrossAppLink to="/settings/acme/projects">Settings</CrossAppLink>);
    expect(screen.getByRole("link", { name: "Settings" })).not.toHaveAttribute("data-router-link");
  });

  it("renders the Shell workspace routes as router links once they are listed", () => {
    // The default list carries the Shell entries; a workspace home and its
    // projects list are web_new screens and navigate in-app.
    render(<CrossAppLink to="/acme/projects">Projects</CrossAppLink>);
    expect(screen.getByRole("link", { name: "Projects" })).toHaveAttribute("data-router-link");
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
    const { rerender } = render(<CrossAppLink to="/acme/active-cycles">Cycles</CrossAppLink>);
    expect(screen.getByRole("link", { name: "Cycles" })).not.toHaveAttribute("data-router-link");
    rerender(
      <CrossAppLink to="/acme/active-cycles" prefixes={["/{ws}/active-cycles"]}>
        Cycles
      </CrossAppLink>
    );
    expect(screen.getByRole("link", { name: "Cycles" })).toHaveAttribute("data-router-link");
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
        to="/acme/projects/1/cycles"
        ref={(element) => {
          seen.push(element);
        }}
      >
        Cycles
      </CrossAppLink>
    );
    expect(seen).toHaveLength(1);
    expect(seen[0]?.tagName).toBe("A");
    expect(seen[0]).toHaveAttribute("href", "/acme/projects/1/cycles");
  });
});
