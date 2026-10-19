// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Identity block (WSET-004/005): initial fallback, upload vs edit action,
// copy control in both roles and the member-dimmed, action-free view.
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { WorkspaceIdentityCard } from "./WorkspaceIdentityCard.js";

afterEach(() => {
  cleanup();
});

function renderCard(overrides: Partial<React.ComponentProps<typeof WorkspaceIdentityCard>> = {}) {
  const props: React.ComponentProps<typeof WorkspaceIdentityCard> = {
    workspaceSlug: "acme",
    workspaceName: "acme workspace",
    logoSrc: null,
    canManage: true,
    onOpenLogo: vi.fn(),
    onCopyAddress: vi.fn(),
    ...overrides,
  };
  render(<WorkspaceIdentityCard {...props} />);
  return props;
}

describe("WorkspaceIdentityCard", () => {
  it("shows the uppercased initial and an Upload logo action without a logo", () => {
    renderCard();
    expect(screen.getByTestId("workspace-general-logo-fallback")).toHaveTextContent("A");
    expect(screen.queryByTestId("workspace-general-logo-image")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Upload logo" })).toBeInTheDocument();
  });

  it("shows the stored image and an Edit logo action", () => {
    renderCard({ logoSrc: "https://cdn.test/logo.png" });
    expect(screen.getByTestId("workspace-general-logo-image")).toHaveAttribute("src", "https://cdn.test/logo.png");
    expect(screen.getByRole("button", { name: "Edit logo" })).toBeInTheDocument();
  });

  it("names the workspace and shows the full address", () => {
    renderCard();
    expect(screen.getByTestId("workspace-general-name")).toHaveTextContent("acme workspace");
    expect(screen.getByText(`${window.location.origin}/acme`)).toBeInTheDocument();
  });

  it("calls the copy and logo handlers", async () => {
    const user = userEvent.setup();
    const props = renderCard();
    await user.click(screen.getByRole("button", { name: /acme$/ }));
    await user.click(screen.getByRole("button", { name: "Upload logo" }));
    expect(props.onCopyAddress).toHaveBeenCalledTimes(1);
    expect(props.onOpenLogo).toHaveBeenCalledTimes(1);
  });

  it("dims the block for members, keeps copy and drops the logo action", () => {
    renderCard({ canManage: false });
    expect(screen.getByTestId("workspace-general-identity").className).toContain("opacity-60");
    expect(screen.queryByRole("button", { name: "Upload logo" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /acme$/ })).toBeInTheDocument();
  });
});
