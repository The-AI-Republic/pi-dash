// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The section creation control settles the install list, the catalog
// counts and every cached calendar window of the project after an install
// (parity/specs/agents.md §3, §5), so the new install shows on whichever
// tab is open without a reload.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const { hooks } = vi.hoisted(() => ({
  hooks: {
    roles: { isSuccess: true, isPending: false, data: { p1: 20 } as Record<string, number> },
  },
}));

vi.mock("../../core/session/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../core/session/index.js")>();
  return { ...original, usePermissions: () => ({ isAdmin: false }) };
});
vi.mock("../auth/queries.js", () => ({
  useProjectRoles: () => hooks.roles,
  useWorkspaceMembership: () => ({ isSuccess: true, isPending: false }),
}));
vi.mock("./installData.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("./installData.js")>();
  return { ...original, useSchedulerBindings: () => ({ data: [] }) };
});
vi.mock("./InstallDialog.js", () => ({
  InstallDialog: ({ onInstalled }: { onInstalled: () => void }) => (
    <button type="button" onClick={onInstalled}>
      Finish install
    </button>
  ),
}));

import { bindingsQueryOptions, schedulerCatalogKey } from "./installData.js";
import { occurrenceKeys } from "./occurrences.js";
import { SectionCreateControl, settleAfterInstall } from "./SectionCreateControl.js";

afterEach(() => {
  hooks.roles = { isSuccess: true, isPending: false, data: { p1: 20 } };
});

function seeded(): QueryClient {
  const client = new QueryClient();
  const week = occurrenceKeys.window("acme", "p1", { from: "2026-11-01T00:00:00Z", to: "2026-11-08T00:00:00Z" });
  const month = occurrenceKeys.window("acme", "p1", { from: "2026-11-01T00:00:00Z", to: "2026-12-01T00:00:00Z" });
  const otherProject = occurrenceKeys.window("acme", "p2", {
    from: "2026-11-01T00:00:00Z",
    to: "2026-11-08T00:00:00Z",
  });
  for (const key of [week, month, otherProject, schedulerCatalogKey("acme")]) client.setQueryData(key, {});
  client.setQueryData(bindingsQueryOptions({} as never, "acme", "p1").queryKey, []);
  return client;
}

function invalidated(client: QueryClient): string[] {
  return client
    .getQueryCache()
    .getAll()
    .filter((query) => query.state.isInvalidated)
    .map((query) => JSON.stringify(query.queryKey));
}

describe("settleAfterInstall", () => {
  it("invalidates the project's installs, the catalog and every calendar window, and nothing else", async () => {
    const client = seeded();
    await settleAfterInstall(client, "acme", "p1");
    const stale = invalidated(client);
    expect(stale).toHaveLength(4);
    expect(stale.filter((key) => key.includes("scheduler-occurrences"))).toHaveLength(2);
    expect(stale.some((key) => key.includes('"p2"'))).toBe(false);
  });
});

describe("SectionCreateControl", () => {
  it("settles the project's views when the dialog reports an install", async () => {
    const client = seeded();
    const user = userEvent.setup();
    render(
      <QueryClientProvider client={client}>
        <SectionCreateControl workspaceSlug="acme" projectId="p1" />
      </QueryClientProvider>
    );
    await user.click(screen.getByRole("button", { name: "Finish install" }));
    await vi.waitFor(() => expect(invalidated(client)).toHaveLength(4));
  });

  it("renders nothing for a caller without project admin standing", () => {
    hooks.roles = { isSuccess: true, isPending: false, data: { p1: 15 } };
    const { container } = render(
      <QueryClientProvider client={new QueryClient()}>
        <SectionCreateControl workspaceSlug="acme" projectId="p1" />
      </QueryClientProvider>
    );
    expect(container).toBeEmptyDOMElement();
  });
});
