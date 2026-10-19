// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Overview screen container (NEWFRONT-540, RUN-002/RUN-005). One screen for
// both scopes: it renders the area's table and sets the document title.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

const useRunnersOverview = vi.fn();

vi.mock("../useOverview.js", () => ({ useRunnersOverview: (slug: string) => useRunnersOverview(slug) }));
vi.mock("../area.js", () => ({
  useRunnersArea: () => ({
    scope: { workspaceSlug: "acme" },
    workspaceId: "ws-id",
    access: "allowed",
    allowed: true,
    base: "/acme/runners",
    path: (child?: string) => (child === undefined ? "/acme/runners" : `/acme/runners/${child}`),
  }),
}));

import { RunnersOverviewScreen, RUNNERS_PAGE_TITLE, runnersDocumentTitle } from "./RunnersOverviewScreen.js";

describe("runnersDocumentTitle", () => {
  it("prefixes the workspace name when known", () => {
    expect(RUNNERS_PAGE_TITLE).toBe("AI Agents");
    expect(runnersDocumentTitle("Acme")).toBe("Acme - AI Agents");
    expect(runnersDocumentTitle(undefined)).toBe("AI Agents");
  });
});

describe("RunnersOverviewScreen", () => {
  it("renders the table from the overview data and loads the workspace's list", () => {
    useRunnersOverview.mockReturnValue({
      selectedPodId: null,
      selectedPod: null,
      runners: [{ id: "r-1", name: "alpha", status: "online", enrolled_at: null, pod_detail: { name: "p" } }],
      toggle: vi.fn(),
      clear: vi.fn(),
      pods: [{ id: "p-1", name: "WEB_alpha" }],
      podsLoading: false,
      podsFailed: false,
      onDelete: vi.fn(async () => undefined),
      onRevoke: vi.fn(async () => undefined),
    });
    render(
      <QueryClientProvider client={new QueryClient()}>
        <RunnersOverviewScreen workspaceSlug="acme" workspaceName="Acme" />
      </QueryClientProvider>
    );
    expect(useRunnersOverview).toHaveBeenCalledWith("acme");
    expect(screen.getByRole("heading", { name: "Runners" })).toBeInTheDocument();
    expect(screen.getByText("alpha")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Filter runners by pod WEB_alpha" })).toBeInTheDocument();
    expect(document.title).toBe("Acme - AI Agents");
  });
});
