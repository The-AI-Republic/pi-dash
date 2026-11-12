// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@tanstack/react-router", () => ({
  Link: ({ to, children, className }: { to: string; children?: React.ReactNode; className?: string }) => (
    <a href={to} className={className}>
      {children}
    </a>
  ),
  useNavigate: () => vi.fn(),
}));

const harness = vi.hoisted(() => ({
  edition: { id: "oss", flags: {} } as {
    id: string;
    flags: Record<string, boolean>;
    slots?: Record<string, unknown>;
  },
}));
vi.mock("../../../core/edition/index.js", () => ({
  get edition() {
    return harness.edition;
  },
}));

import { makeView } from "../testing.js";
import { ProjectViewRowItem } from "./ProjectViewRowItem.js";

const PROJECT = "323e4567-e89b-12d3-a456-426614174002";
const OWNER = "423e4567-e89b-12d3-a456-426614174003";

function renderItem(anchor: string | null = null): void {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ul>
        <ProjectViewRowItem
          workspaceSlug="acme"
          projectId={PROJECT}
          view={makeView({ name: "Triage", project: PROJECT, owned_by: OWNER, anchor })}
          owner={{ name: "Ada Lovelace", avatarUrl: null }}
          viewerId={OWNER}
          projectRole={15}
          hideCycle={false}
          hideModule={false}
        />
      </ul>
    </QueryClientProvider>
  );
}

afterEach(() => {
  harness.edition = { id: "oss", flags: {} };
});

describe("ProjectViewRowItem", () => {
  it("mounts the star and the menu beside the row identity", () => {
    renderItem();

    expect(screen.getByTestId("project-views-row-name")).toHaveTextContent("Triage");
    expect(screen.getByTestId("project-views-row-star")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Actions for Triage" })).toBeInTheDocument();
  });

  it("opens the publish dialog from the Live marker on cloud", async () => {
    harness.edition = {
      id: "cloud",
      flags: {},
      slots: {
        viewPublish: {
          publish: vi.fn(),
          unpublish: vi.fn(),
          publicLink: (marker: string) => `https://public.test/v/${marker}`,
        },
      },
    };
    renderItem("pub-1");

    fireEvent.click(screen.getByRole("button", { name: "Public link for Triage" }));
    expect(await screen.findByRole("heading", { name: "Published view" })).toBeInTheDocument();
    expect(screen.getByTestId("publish-view-link")).toHaveTextContent("https://public.test/v/pub-1");
  });

  it("leaves the marker a plain badge on OSS", async () => {
    renderItem("pub-1");

    expect(screen.getByTestId("project-views-row-live")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Public link/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Actions for Triage" }));
    await waitFor(() => expect(screen.getAllByRole("menuitem").length).toBeGreaterThan(0));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).not.toContain("Publish");
  });
});
