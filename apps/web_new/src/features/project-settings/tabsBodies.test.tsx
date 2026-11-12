// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Tab bodies (PSET-049–052): the tab shells pass the shell's project,
// workspace and effective standing through to their surfaces.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  screenProps: [] as { workspaceSlug: string; projectId: string; canManage: boolean }[],
  cardProps: [] as { workspaceSlug: string; projectId: string; canManage: boolean }[],
  context: {
    role: 20 as number | null,
    project: null as { name: string; logoProps: Record<string, unknown>; identifier: string } | null,
  },
}));

vi.mock("../estimates/EstimatesScreen.js", () => ({
  EstimatesScreen: (props: { workspaceSlug: string; projectId: string; canManage: boolean }) => {
    harness.screenProps.push(props);
    return <div data-testid="estimates-screen-stub" />;
  },
}));

vi.mock("./repository/RepositorySyncCard.js", () => ({
  RepositorySyncCard: (props: { workspaceSlug: string; projectId: string; canManage: boolean }) => {
    harness.cardProps.push(props);
    return <div data-testid="repository-card-stub" />;
  },
}));

vi.mock("./components/ProjectSettingsLayout.js", () => ({
  useProjectSettings: () => ({
    workspaceSlug: "acme",
    projectId: "223e4567-e89b-12d3-a456-426614174001",
    role: harness.context.role,
    workspaceAdmin: false,
    project: harness.context.project,
  }),
}));

import { EstimatesTab, estimatesTabTitle } from "./estimates/EstimatesTab.js";
import { RepositoryTab } from "./repository/RepositoryTab.js";

function wrap(node: React.ReactElement): React.ReactElement {
  return <QueryClientProvider client={new QueryClient()}>{node as never}</QueryClientProvider>;
}

beforeEach(() => {
  harness.screenProps = [];
  harness.cardProps = [];
  harness.context.role = 20;
  harness.context.project = null;
  document.title = "";
});

describe("settings tab bodies", () => {
  it("hands the estimates surface the project and admin standing", () => {
    render(wrap(<EstimatesTab />));
    expect(screen.getByTestId("estimates-screen-stub")).toBeInTheDocument();
    expect(harness.screenProps).toEqual([
      { workspaceSlug: "acme", projectId: "223e4567-e89b-12d3-a456-426614174001", canManage: true },
    ]);
  });

  it("does not grant estimate management to a member", () => {
    harness.context.role = 15;
    render(wrap(<EstimatesTab />));
    expect(harness.screenProps[0]?.canManage).toBe(false);
  });

  it("titles the estimates tab with the project name", () => {
    harness.context.project = { name: "Website relaunch", logoProps: {}, identifier: "WEB" };
    render(wrap(<EstimatesTab />));
    expect(screen.getByTestId("estimates-screen-stub")).toBeInTheDocument();
    expect(document.title).toBe("Website relaunch - Estimates");
    expect(document.title).toContain("Website relaunch");
  });

  it("falls back to the area title while the project is unresolved", () => {
    expect(estimatesTabTitle(null)).toBe("Estimates");
    render(wrap(<EstimatesTab />));
    expect(document.title).toBe("Estimates");
  });

  it("hands the repository card the project and admin standing", () => {
    render(wrap(<RepositoryTab />));
    expect(screen.getByTestId("repository-card-stub")).toBeInTheDocument();
    expect(harness.cardProps).toEqual([
      { workspaceSlug: "acme", projectId: "223e4567-e89b-12d3-a456-426614174001", canManage: true },
    ]);
  });

  it("keeps the repository card visible but read-only for a member", () => {
    harness.context.role = 15;
    render(wrap(<RepositoryTab />));
    expect(harness.cardProps[0]?.canManage).toBe(false);
  });
});
