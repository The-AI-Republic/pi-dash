// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Modules tab body (parity/specs/modules.md §10.1, MOD-036): the tab is the
// project-settings switch for the modules flag, and a successful flip
// refreshes every modules read that gates on that flag so returning to the
// list shows the new state without a reload. A refused write refreshes
// nothing.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  project: { module_view: false } as Record<string, boolean> | undefined,
  mutate: vi.fn(),
  settle: vi.fn(),
}));

vi.mock("../../../features/project-settings/index.js", () => ({
  useProjectSettings: () => ({ workspaceSlug: "acme", projectId: "p1", role: 20, project: null }),
}));

vi.mock("../../../features/auth/index.js", () => ({
  useProjectRow: () => ({ data: harness.project, isPending: false }),
}));

vi.mock("../../../shared/shell/index.js", () => ({
  notifyPending: () => ({ id: "notice", settle: harness.settle }),
}));

vi.mock("../../project-features/api/featureQueries.js", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useUpdateProjectFeature: () => ({ mutate: harness.mutate, isPending: false }),
}));

import { moduleKeys } from "../api/keys.js";
import { ModuleFeatureSettings } from "./ModuleFeatureSettings.js";

function renderModuleTab(): QueryClient {
  const client = new QueryClient();
  render(
    <QueryClientProvider client={client}>
      <ModuleFeatureSettings workspaceSlug="acme" projectId="p1" />
    </QueryClientProvider>
  );
  return client;
}

beforeEach(() => {
  harness.project = { module_view: false };
  harness.mutate.mockReset();
  harness.settle.mockReset();
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("ModuleFeatureSettings", () => {
  it("renders the project-settings switch for the modules flag", () => {
    renderModuleTab();
    const toggle = screen.getByRole("switch", { name: "Enable modules" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(screen.getByTestId("project-feature-settings")).toHaveAttribute("data-feature", "modules");
  });

  it("mirrors a flag already on", () => {
    harness.project = { module_view: true };
    renderModuleTab();
    expect(screen.getByRole("switch", { name: "Enable modules" })).toHaveAttribute("aria-checked", "true");
  });

  it("writes the inverted flag and refreshes the modules reads on success", async () => {
    const user = userEvent.setup();
    const client = renderModuleTab();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    await user.click(screen.getByRole("switch", { name: "Enable modules" }));
    expect(harness.mutate).toHaveBeenCalledWith(
      { projectId: "p1", patch: { module_view: true } },
      expect.objectContaining({ onSuccess: expect.any(Function), onError: expect.any(Function) })
    );
    const [, options] = harness.mutate.mock.calls[0] as [unknown, { onSuccess: () => void }];
    options.onSuccess();
    await waitFor(() =>
      expect(invalidate.mock.calls.map(([arg]) => arg?.queryKey)).toEqual(
        expect.arrayContaining([
          moduleKeys.list("acme", "p1"),
          moduleKeys.workspaceList("acme"),
          moduleKeys.projectFacts("acme", "p1"),
        ])
      )
    );
  });

  it("refreshes nothing when the write is refused", async () => {
    const user = userEvent.setup();
    const client = renderModuleTab();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    await user.click(screen.getByRole("switch", { name: "Enable modules" }));
    const [, options] = harness.mutate.mock.calls[0] as [unknown, { onError: () => void }];
    options.onError();
    expect(invalidate).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: "Enable modules" })).toHaveAttribute("aria-checked", "false")
    );
  });
});
