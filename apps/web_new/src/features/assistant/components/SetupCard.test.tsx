// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Setup card (AGT-039): explains the bring-your-own-key rule and the
// permission scope, and links the provider settings page.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

// Stand-in for the router Link (CrossAppLink.test.tsx precedent): the
// setup action targets a migrated prefix, so it renders as a router
// link whose navigation is TanStack's own tested behavior.
vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
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

import { SetupCard } from "./SetupCard.js";

describe("SetupCard", () => {
  it("states the key rule and permission scope", () => {
    render(<SetupCard workspaceSlug="acme" />);
    const card = screen.getByTestId("assistant-setup-card");
    expect(card).toHaveAccessibleName("Set up your AI assistant");
    expect(screen.getByRole("heading", { name: "Set up your AI assistant" })).toBeInTheDocument();
    expect(card).toHaveTextContent("Bring your own LLM provider key");
    expect(card).toHaveTextContent("exactly your permissions");
  });

  it("links the provider settings page with the pinned action label", () => {
    render(<SetupCard workspaceSlug="acme" />);
    const action = screen.getByRole("button", { name: "Configure provider" });
    expect(action).toHaveAttribute("data-router-link");
    expect(action).toHaveAttribute("data-to", "/acme/settings/account/ai-assistant/");
  });

  it("forwards layout classes", () => {
    render(<SetupCard workspaceSlug="acme" className="extra-class" />);
    expect(screen.getByTestId("assistant-setup-card")).toHaveClass("extra-class");
  });
});
