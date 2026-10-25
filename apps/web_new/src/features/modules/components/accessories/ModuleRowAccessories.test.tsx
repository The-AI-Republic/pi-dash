// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The list row's inline accessory group (parity/specs/modules.md §4,
// MOD-013–016): dates, status, lead and star for an editor on a live
// module; the date and status controls disabled and no star for guests
// and archived rows. The lead resolves against the directory the host
// passes.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import type { ModuleRow, WorkspaceMember } from "@pidash/api-client";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../../../core/api/client.js", () => ({ getAppClient: () => ({ stub: true }) }));

vi.mock("../../../issues/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, useWeekStart: () => 0 };
});

import { makeDirectoryEntry, makeModuleRow, MODULES_LEAD_ID, MODULES_PROJECT_ID } from "../../testing.js";
import { ModuleRowAccessories } from "./ModuleRowAccessories.js";

function renderAccessories(
  module: ModuleRow,
  standing: { memberRole: number | null; archived: boolean },
  members: readonly WorkspaceMember[] | undefined = undefined
): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <ModuleRowAccessories
        workspaceSlug="acme"
        projectId={MODULES_PROJECT_ID}
        module={module}
        memberRole={standing.memberRole}
        archived={standing.archived}
        members={members}
      />
    </QueryClientProvider>
  );
}

describe("ModuleRowAccessories", () => {
  it("offers every accessory enabled to an editor on a live module", () => {
    renderAccessories(makeModuleRow(), { memberRole: 15, archived: false });
    const group = screen.getByTestId("modules-row-accessories");
    expect(within(group).getByTestId("modules-row-dates-trigger")).toBeEnabled();
    expect(within(group).getByTestId("modules-row-status")).toBeEnabled();
    expect(within(group).getByTestId("modules-row-lead")).toHaveAttribute("data-lead-kind", "none");
    expect(within(group).getByTestId("modules-row-star")).toBeInTheDocument();
  });

  it("resolves the lead avatar against the directory", () => {
    renderAccessories(makeModuleRow({ lead_id: MODULES_LEAD_ID }), { memberRole: 20, archived: false }, [
      makeDirectoryEntry(),
    ]);
    const lead = screen.getByTestId("modules-row-lead");
    expect(lead).toHaveAttribute("data-lead-kind", "avatar");
    expect(within(lead).getByText("AL")).toBeInTheDocument();
  });

  it("disables the controls and hides the star for a guest", () => {
    renderAccessories(makeModuleRow(), { memberRole: 5, archived: false });
    expect(screen.getByTestId("modules-row-dates-trigger")).toBeDisabled();
    expect(screen.getByTestId("modules-row-status")).toBeDisabled();
    expect(screen.getByTestId("modules-row-lead")).toBeInTheDocument();
    expect(screen.queryByTestId("modules-row-star")).not.toBeInTheDocument();
  });

  it("disables the controls and hides the star on an archived row", () => {
    renderAccessories(makeModuleRow(), { memberRole: 15, archived: true });
    expect(screen.getByTestId("modules-row-dates-trigger")).toBeDisabled();
    expect(screen.getByTestId("modules-row-status")).toBeDisabled();
    expect(screen.queryByTestId("modules-row-star")).not.toBeInTheDocument();
  });

  it("reads an unknown standing as read-only", () => {
    renderAccessories(makeModuleRow(), { memberRole: null, archived: false });
    expect(screen.getByTestId("modules-row-dates-trigger")).toBeDisabled();
    expect(screen.getByTestId("modules-row-status")).toBeDisabled();
    expect(screen.queryByTestId("modules-row-star")).not.toBeInTheDocument();
  });
});
