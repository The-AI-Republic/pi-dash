// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Feature tab body (PSET-043–047): the switch mirrors the loaded project,
// each press writes one boolean under a pending notice settled into its
// outcome, and a refused write keeps the stored value.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  project: { issue_views_view: false, cycle_view: true, inbox_view: false, module_view: false, page_view: true } as
    | Record<string, boolean>
    | undefined,
  pending: false,
  mutate: vi.fn(),
}));

vi.mock("../../../features/project-settings/index.js", () => ({
  useProjectSettings: () => ({ workspaceSlug: "acme", projectId: "p1", role: 20, project: null }),
}));

vi.mock("../../../features/auth/index.js", () => ({
  useProjectRow: () => ({ data: harness.project, isPending: harness.pending }),
}));

const mocked = vi.hoisted(() => ({ settle: vi.fn(), notifyPending: vi.fn() }));

vi.mock("../../../shared/shell/index.js", () => ({
  notifyPending: mocked.notifyPending,
}));

vi.mock("../api/featureQueries.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useUpdateProjectFeature: () => ({ mutate: harness.mutate, isPending: harness.pending }),
}));

import { notifyPending } from "../../../shared/shell/index.js";
import { PROJECT_FEATURES, PROJECT_FEATURE_LIST, type ProjectFeatureKey } from "../lib/registry.js";
import { ProjectFeatureSettings } from "./ProjectFeatureSettings.js";

function renderFeature(key: ProjectFeatureKey): void {
  const feature = PROJECT_FEATURES[key];
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ProjectFeatureSettings feature={feature} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  harness.project = {
    issue_views_view: false,
    cycle_view: true,
    inbox_view: false,
    module_view: false,
    page_view: true,
  };
  harness.pending = false;
  harness.mutate.mockReset();
  mocked.settle.mockReset();
  mocked.notifyPending.mockReset();
  mocked.notifyPending.mockImplementation(() => ({ id: "t1", settle: mocked.settle }));
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("ProjectFeatureSettings", () => {
  it("mirrors the loaded project flag and names the switch", () => {
    renderFeature("views");
    expect(screen.getByRole("heading", { name: "Enable views" })).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Enable views" })).toHaveAttribute("aria-checked", "false");
    expect(screen.getByTestId("project-feature-settings")).toHaveAttribute("data-feature", "views");
    expect(screen.getByText(PROJECT_FEATURES.views.explainer)).toBeInTheDocument();
  });

  it("renders every one of the five tabs with its own switch", () => {
    for (const feature of PROJECT_FEATURE_LIST) {
      const { unmount } = render(
        <QueryClientProvider client={new QueryClient()}>
          <ProjectFeatureSettings feature={feature} />
        </QueryClientProvider>
      );
      expect(screen.getByRole("switch", { name: feature.switchLabel })).toBeInTheDocument();
      unmount();
    }
  });

  it("shows an absent project row as off rather than loading", () => {
    harness.project = undefined;
    renderFeature("modules");
    expect(screen.getByRole("switch", { name: "Enable modules" })).toHaveAttribute("aria-checked", "false");
  });

  it("writes one inverted flag under a pending notice", async () => {
    const user = userEvent.setup();
    renderFeature("views");
    await user.click(screen.getByRole("switch", { name: "Enable views" }));
    expect(notifyPending).toHaveBeenCalledWith("Saving the switch", "This will only take a moment.");
    expect(harness.mutate).toHaveBeenCalledWith(
      { projectId: "p1", patch: { issue_views_view: true } },
      expect.objectContaining({ onSuccess: expect.any(Function), onError: expect.any(Function) })
    );
  });

  it("writes off for a flag already on", async () => {
    const user = userEvent.setup();
    renderFeature("cycles");
    await user.click(screen.getByRole("switch", { name: "Enable cycles" }));
    expect(harness.mutate).toHaveBeenCalledWith({ projectId: "p1", patch: { cycle_view: false } }, expect.anything());
  });

  it("settles the pending notice on success", async () => {
    const user = userEvent.setup();
    renderFeature("intake");
    await user.click(screen.getByRole("switch", { name: "Enable intake" }));
    const [, options] = harness.mutate.mock.calls[0] as [unknown, { onSuccess: () => void }];
    options.onSuccess();
    expect(mocked.settle).toHaveBeenCalledWith({
      title: "Project feature updated",
      description: "The change applies across this project right away.",
    });
  });

  it("settles the pending notice as a failure without touching the row", async () => {
    const user = userEvent.setup();
    renderFeature("pages");
    await user.click(screen.getByRole("switch", { name: "Enable pages" }));
    const [, options] = harness.mutate.mock.calls[0] as [unknown, { onError: () => void }];
    options.onError();
    expect(mocked.settle).toHaveBeenCalledWith({
      title: "Error!",
      description: "The switch could not be saved. Its stored value is still in place.",
      failed: true,
    });
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "Enable pages" })).toHaveAttribute("aria-checked", "true")
    );
  });

  it("locks the switch while its own write is in flight", () => {
    harness.pending = true;
    renderFeature("modules");
    expect(screen.getByRole("switch", { name: "Enable modules" })).toHaveAttribute("aria-disabled", "true");
  });
});
