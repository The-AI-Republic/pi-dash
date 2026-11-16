// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

// Stand-in for the router Link: navigation is TanStack's own tested
// behavior. What this suite owns is the row identity.
vi.mock("@tanstack/react-router", () => ({
  Link: ({ to, children, className }: { to: string; children?: React.ReactNode; className?: string }) => (
    <a href={to} className={className}>
      {children}
    </a>
  ),
}));

import { makeView } from "../testing.js";
import { ProjectViewRow } from "./ProjectViewRow.js";

const OWNER = { name: "Ada Lovelace", avatarUrl: "https://cdn.example/ada.png" };

function renderRow(
  view: Parameters<typeof ProjectViewRow>[0]["view"],
  owner: { name: string; avatarUrl: string | null } | null = OWNER
) {
  return render(<ProjectViewRow workspaceSlug="acme" projectId="project-1" view={view} owner={owner} />);
}

describe("ProjectViewRow", () => {
  it("links the whole identity cluster to the detail page", () => {
    const view = makeView({ id: "view-9", name: "Triage" });
    renderRow(view);
    const row = screen.getByTestId("project-views-row");
    expect(row).toHaveAttribute("data-view-id", "view-9");
    const link = screen.getByRole("link", { name: /Triage/ });
    expect(link).toHaveAttribute("href", "/acme/projects/project-1/views/view-9");
    expect(link).toHaveTextContent("Triage");
  });

  it("renders the stored emoji logo as a glyph", () => {
    const view = makeView({ logo_props: { in_use: "emoji", emoji: { value: "128640" } } });
    renderRow(view);
    expect(screen.getByTestId("project-views-row")).toHaveTextContent("🚀");
  });

  it("renders the stored glyph icon once its chunk arrives", async () => {
    const view = makeView({ name: "Triage", logo_props: { in_use: "icon", icon: { name: "anchor", color: null } } });
    renderRow(view);
    await waitFor(() => {
      expect(screen.getByTestId("project-views-row").querySelector("svg")).not.toBeNull();
    });
  });

  it("falls back to the default glyph without a custom logo", () => {
    renderRow(makeView({ logo_props: {} }));
    expect(screen.getByTestId("project-views-row").querySelector("svg")).not.toBeNull();
  });

  it("badges shared views Public and owner-only views Private", () => {
    const { rerender } = renderRow(makeView({ access: 1 }));
    expect(screen.getByTestId("project-views-row-access")).toHaveTextContent("Public");
    rerender(
      <ProjectViewRow workspaceSlug="acme" projectId="project-1" view={makeView({ access: 0 })} owner={OWNER} />
    );
    expect(screen.getByTestId("project-views-row-access")).toHaveTextContent("Private");
  });

  it("marks Live only when the view carries an anchor", () => {
    const { rerender } = renderRow(makeView({ anchor: "anchor-token" }));
    expect(screen.getByTestId("project-views-row-live")).toHaveTextContent("Live");
    rerender(
      <ProjectViewRow workspaceSlug="acme" projectId="project-1" view={makeView({ anchor: null })} owner={OWNER} />
    );
    expect(screen.queryByTestId("project-views-row-live")).not.toBeInTheDocument();
    rerender(<ProjectViewRow workspaceSlug="acme" projectId="project-1" view={makeView({})} owner={OWNER} />);
    expect(screen.queryByTestId("project-views-row-live")).not.toBeInTheDocument();
  });

  it("opens the publish dialog from the Live marker instead of navigating", () => {
    const onLiveMarkerClick = vi.fn();
    const view = makeView({ name: "Triage", anchor: "anchor-token" });
    render(
      <ProjectViewRow
        workspaceSlug="acme"
        projectId="project-1"
        view={view}
        owner={OWNER}
        onLiveMarkerClick={onLiveMarkerClick}
      />
    );
    const marker = screen.getByRole("button", { name: "Public link for Triage" });
    expect(marker).toHaveTextContent("Live");
    fireEvent.click(marker);
    expect(onLiveMarkerClick).toHaveBeenCalledTimes(1);
  });

  it("keeps the marker a plain badge without a publish opener", () => {
    renderRow(makeView({ anchor: "anchor-token" }));
    const marker = screen.getByTestId("project-views-row-live");
    expect(marker.tagName).toBe("SPAN");
    expect(screen.queryByRole("button", { name: /Public link/ })).not.toBeInTheDocument();
  });

  it("tags the row name for the parity driver", () => {
    renderRow(makeView({ name: "Triage" }));
    expect(screen.getByTestId("project-views-row-name")).toHaveTextContent("Triage");
  });

  it("shows the owner avatar and skips it when the owner is unknown", () => {
    const { rerender } = renderRow(makeView(), OWNER);
    expect(screen.getByTestId("project-views-row-owner")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Ada Lovelace" })).toBeInTheDocument();
    rerender(
      <ProjectViewRow
        workspaceSlug="acme"
        projectId="project-1"
        view={makeView()}
        owner={{ name: "Ada Lovelace", avatarUrl: null }}
      />
    );
    expect(screen.getByTestId("project-views-row-owner")).toHaveTextContent("AL");
    rerender(<ProjectViewRow workspaceSlug="acme" projectId="project-1" view={makeView()} owner={null} />);
    expect(screen.queryByTestId("project-views-row-owner")).not.toBeInTheDocument();
  });

  it("mounts the NEWFRONT-722 trailing slot beside the link", () => {
    render(
      <ProjectViewRow
        workspaceSlug="acme"
        projectId="project-1"
        view={makeView()}
        owner={OWNER}
        trailingControl={<span data-testid="slot-actions" />}
      />
    );
    const slot = screen.getByTestId("slot-actions");
    expect(slot).toBeInTheDocument();
    expect(slot.closest("a")).toBeNull();
  });
});
