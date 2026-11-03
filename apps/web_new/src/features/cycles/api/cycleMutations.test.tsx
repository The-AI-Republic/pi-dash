// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycle delete/archive/restore mutations (parity/specs/cycles.md §8
// and §9, CYC-022, CYC-024, CYC-026): toasts, refresh, navigation,
// the permission mapping, and honest failures that leave the row.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
  type AnyRouter,
} from "@tanstack/react-router";
import { act, render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const notify = vi.hoisted(() => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));
vi.mock("../../../shared/shell/toaster.js", () => ({
  notifyError: notify.notifyError,
  notifySuccess: notify.notifySuccess,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { cycleKeys } from "./keys.js";
import {
  CYCLE_ARCHIVE_FAILED_TITLE,
  CYCLE_ARCHIVE_LINE,
  CYCLE_ARCHIVE_TITLE,
  CYCLE_DELETED_TITLE,
  CYCLE_DELETE_FAILED_TITLE,
  CYCLE_PERMISSION_TITLE,
  CYCLE_RESTORED_LINE,
  CYCLE_RESTORED_TITLE,
  CYCLE_RESTORE_FAILED_TITLE,
  useArchiveCycle,
  useDeleteCycle,
  useRestoreCycle,
  type CycleMutationTarget,
} from "./cycleMutations.js";
import { CYCLES_CYCLE_ID, CYCLES_PROJECT_ID } from "../testing.js";

const TARGET: CycleMutationTarget = {
  workspaceSlug: "acme",
  projectId: CYCLES_PROJECT_ID,
  cycleId: CYCLES_CYCLE_ID,
};
const LIST_PATH = `/acme/projects/${CYCLES_PROJECT_ID}/cycles`;
const DETAIL_PATH = `${LIST_PATH}/${CYCLES_CYCLE_ID}`;

interface Harness {
  deleteStatus: number;
  archiveStatus: number;
  restoreStatus: number;
  deletes: number;
  archives: number;
  restores: number;
  holdWrites: boolean;
  gate: (() => void) | null;
  releaseWrites: () => void;
}

let harness: Harness;
let queryClient: QueryClient;
let router: AnyRouter;

function resetHarness(): Harness {
  const state: Harness = {
    deleteStatus: 204,
    archiveStatus: 200,
    restoreStatus: 204,
    deletes: 0,
    archives: 0,
    restores: 0,
    holdWrites: false,
    gate: null,
    releaseWrites: () => state.gate?.(),
  };
  return state;
}

function transport(url: string, init: { method?: string; body?: string }): Promise<unknown> {
  const method = init.method ?? "GET";
  const cycleUrl = `https://api.test/api/workspaces/acme/projects/${CYCLES_PROJECT_ID}/cycles/${CYCLES_CYCLE_ID}/`;
  const archiveUrl = `${cycleUrl}archive/`;
  const respond = async (status: number, body: unknown): Promise<unknown> => {
    if (harness.holdWrites) await new Promise<void>((release) => (harness.gate = release));
    return { status, url, headers: { get: () => null }, text: async () => JSON.stringify(body) };
  };
  if (method === "DELETE" && url === archiveUrl) {
    harness.restores += 1;
    return respond(harness.restoreStatus, harness.restoreStatus === 204 ? null : { detail: "boom" });
  }
  if (method === "DELETE" && url === cycleUrl) {
    harness.deletes += 1;
    return respond(
      harness.deleteStatus,
      harness.deleteStatus === 204 ? null : { error: "You don't have the required permissions." }
    );
  }
  if (method === "POST" && url === archiveUrl) {
    harness.archives += 1;
    return respond(
      harness.archiveStatus,
      harness.archiveStatus === 200 ? { archived_at: "2026-03-01T00:00:00Z" } : { detail: "boom" }
    );
  }
  throw new Error(`unexpected request ${method} ${url}`);
}

interface ProbeApi {
  run: () => Promise<boolean>;
  pending: () => boolean;
}

function DeleteProbe({ api }: { api: { current: ProbeApi | null } }): React.ReactElement {
  const { remove, pending } = useDeleteCycle(TARGET);
  api.current = { run: remove, pending: () => pending };
  return <p>probe</p>;
}

function ArchiveProbe({ api }: { api: { current: ProbeApi | null } }): React.ReactElement {
  const { archive, pending } = useArchiveCycle(TARGET);
  api.current = { run: archive, pending: () => pending };
  return <p>probe</p>;
}

function RestoreProbe({ api }: { api: { current: ProbeApi | null } }): React.ReactElement {
  const { restore, pending } = useRestoreCycle(TARGET);
  api.current = { run: restore, pending: () => pending };
  return <p>probe</p>;
}

async function renderProbe(
  probe: (api: { current: ProbeApi | null }) => React.ReactElement,
  initialEntry: string
): Promise<{ current: ProbeApi | null }> {
  const api: { current: ProbeApi | null } = { current: null };
  const rootRoute = createRootRoute({
    component: () => (
      <>
        <Outlet />
        {probe(api)}
      </>
    ),
  });
  const listRoute = createRoute({ getParentRoute: () => rootRoute, path: LIST_PATH, component: () => <p>list</p> });
  const detailRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: DETAIL_PATH,
    component: () => <p>detail</p>,
  });
  router = createRouter({
    routeTree: rootRoute.addChildren([listRoute, detailRoute]),
    history: createMemoryHistory({ initialEntries: [initialEntry] }),
    defaultPreload: false,
  });
  render(
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  );
  await screen.findByText("probe");
  return api;
}

function seedListQuery(): void {
  queryClient.setQueryData(cycleKeys.list("acme", CYCLES_PROJECT_ID), []);
}

function listInvalidated(): boolean {
  return queryClient.getQueryState(cycleKeys.list("acme", CYCLES_PROJECT_ID))?.isInvalidated === true;
}

beforeEach(() => {
  harness = resetHarness();
  queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  createAppClient("https://api.test", transport as never);
  // jsdom never implemented scrolling; the router's scroll restoration
  // would otherwise log a not-implemented error on every navigation.
  Object.defineProperty(window, "scrollTo", { value: () => undefined, writable: true });
});

/** Wait until the held write reaches the transport, then release it. */
async function releaseHeldWrite(): Promise<void> {
  await vi.waitFor(() => expect(harness.gate).not.toBeNull());
  harness.releaseWrites();
}

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
});

describe("useDeleteCycle", () => {
  it("deletes from detail, confirms, refreshes, and returns to the list", async () => {
    seedListQuery();
    const api = await renderProbe((handle) => <DeleteProbe api={handle} />, DETAIL_PATH);
    let ok = false;
    await act(async () => {
      ok = await api.current!.run();
    });
    expect(ok).toBe(true);
    expect(harness.deletes).toBe(1);
    expect(notify.notifySuccess).toHaveBeenCalledWith(CYCLE_DELETED_TITLE);
    expect(listInvalidated()).toBe(true);
    expect(router.state.location.pathname).toBe(LIST_PATH);
    expect(await screen.findByText("list")).toBeInTheDocument();
  });

  it("drops the peek key when deleting with the quick look open", async () => {
    const api = await renderProbe(
      (handle) => <DeleteProbe api={handle} />,
      `${LIST_PATH}?peekCycle=${CYCLES_CYCLE_ID}`
    );
    await act(async () => {
      await api.current!.run();
    });
    expect(router.state.location.pathname).toBe(LIST_PATH);
    expect(router.state.location.search).toEqual({});
  });

  it("maps a 403 to the permission message and leaves the row", async () => {
    harness.deleteStatus = 403;
    seedListQuery();
    const api = await renderProbe((handle) => <DeleteProbe api={handle} />, LIST_PATH);
    let ok = true;
    await act(async () => {
      ok = await api.current!.run();
    });
    expect(ok).toBe(false);
    expect(notify.notifyError).toHaveBeenCalledWith(CYCLE_PERMISSION_TITLE);
    expect(notify.notifyError).not.toHaveBeenCalledWith(CYCLE_DELETE_FAILED_TITLE, expect.anything());
    expect(listInvalidated()).toBe(false);
    expect(router.state.location.pathname).toBe(LIST_PATH);
  });

  it("maps other failures to the generic message without navigating", async () => {
    harness.deleteStatus = 500;
    const api = await renderProbe((handle) => <DeleteProbe api={handle} />, DETAIL_PATH);
    let ok = true;
    await act(async () => {
      ok = await api.current!.run();
    });
    expect(ok).toBe(false);
    expect(notify.notifyError).toHaveBeenCalledWith(CYCLE_DELETE_FAILED_TITLE, expect.any(String));
    expect(notify.notifySuccess).not.toHaveBeenCalled();
    expect(router.state.location.pathname).toBe(DETAIL_PATH);
  });

  it("sends one write for overlapping calls", async () => {
    harness.holdWrites = true;
    const api = await renderProbe((handle) => <DeleteProbe api={handle} />, LIST_PATH);
    let first = false;
    let second = true;
    await act(async () => {
      const pending = api.current!.run();
      const duplicate = api.current!.run();
      await releaseHeldWrite();
      first = await pending;
      second = await duplicate;
    });
    expect(first).toBe(true);
    expect(second).toBe(false);
    expect(harness.deletes).toBe(1);
    expect(api.current!.pending()).toBe(false);
  });
});

describe("useArchiveCycle", () => {
  it("archives through the stamp write, confirms, and returns to the list", async () => {
    seedListQuery();
    const api = await renderProbe((handle) => <ArchiveProbe api={handle} />, DETAIL_PATH);
    let ok = false;
    await act(async () => {
      ok = await api.current!.run();
    });
    expect(ok).toBe(true);
    expect(harness.archives).toBe(1);
    expect(notify.notifySuccess).toHaveBeenCalledWith(CYCLE_ARCHIVE_TITLE, CYCLE_ARCHIVE_LINE);
    expect(listInvalidated()).toBe(true);
    expect(router.state.location.pathname).toBe(LIST_PATH);
  });

  it("reports a failed archive and stays put", async () => {
    harness.archiveStatus = 500;
    seedListQuery();
    const api = await renderProbe((handle) => <ArchiveProbe api={handle} />, LIST_PATH);
    let ok = true;
    await act(async () => {
      ok = await api.current!.run();
    });
    expect(ok).toBe(false);
    expect(notify.notifyError).toHaveBeenCalledWith(CYCLE_ARCHIVE_FAILED_TITLE, expect.any(String));
    expect(notify.notifySuccess).not.toHaveBeenCalled();
    expect(listInvalidated()).toBe(false);
    expect(router.state.location.pathname).toBe(LIST_PATH);
  });

  it("sends one write for overlapping calls", async () => {
    harness.holdWrites = true;
    const api = await renderProbe((handle) => <ArchiveProbe api={handle} />, LIST_PATH);
    await act(async () => {
      const pending = api.current!.run();
      const duplicate = api.current!.run();
      await releaseHeldWrite();
      expect(await pending).toBe(true);
      expect(await duplicate).toBe(false);
    });
    expect(harness.archives).toBe(1);
  });
});

describe("useRestoreCycle", () => {
  it("restores without navigating away", async () => {
    seedListQuery();
    const api = await renderProbe((handle) => <RestoreProbe api={handle} />, LIST_PATH);
    let ok = false;
    await act(async () => {
      ok = await api.current!.run();
    });
    expect(ok).toBe(true);
    expect(harness.restores).toBe(1);
    expect(notify.notifySuccess).toHaveBeenCalledWith(CYCLE_RESTORED_TITLE, CYCLE_RESTORED_LINE);
    expect(listInvalidated()).toBe(true);
    expect(router.state.location.pathname).toBe(LIST_PATH);
  });

  it("reports a failed restore and keeps the cycle archived", async () => {
    harness.restoreStatus = 404;
    seedListQuery();
    const api = await renderProbe((handle) => <RestoreProbe api={handle} />, LIST_PATH);
    let ok = true;
    await act(async () => {
      ok = await api.current!.run();
    });
    expect(ok).toBe(false);
    expect(notify.notifyError).toHaveBeenCalledWith(CYCLE_RESTORE_FAILED_TITLE, expect.any(String));
    expect(notify.notifySuccess).not.toHaveBeenCalled();
    expect(listInvalidated()).toBe(false);
  });

  it("sends one write for overlapping calls", async () => {
    harness.holdWrites = true;
    const api = await renderProbe((handle) => <RestoreProbe api={handle} />, LIST_PATH);
    await act(async () => {
      const pending = api.current!.run();
      const duplicate = api.current!.run();
      await releaseHeldWrite();
      expect(await pending).toBe(true);
      expect(await duplicate).toBe(false);
    });
    expect(harness.restores).toBe(1);
  });
});
