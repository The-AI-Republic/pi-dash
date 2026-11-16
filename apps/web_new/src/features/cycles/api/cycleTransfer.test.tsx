// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Transfer write (parity/specs/cycles.md §12, CYC-031): the move posts
// the destination, confirms with the pinned copy, refreshes both sides
// plus both work-item lists, and reports a refusal without moving. The
// source row re-read clears its transfer entry and freezes its ring on
// the written snapshot without a reload (NEWFRONT-267, CYC-045).

import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { CycleRow } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const notify = vi.hoisted(() => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));
vi.mock("../../../shared/shell/toaster.js", () => ({
  notifyError: notify.notifyError,
  notifySuccess: notify.notifySuccess,
}));

import { createAppClient, getAppClient, resetAppClient } from "../../../core/api/client.js";
import { canTransferFromBanner, canTransferFromRow } from "../lib/cycleTransfer.js";
import { cycleProgressPercent } from "../lib/cycleProgress.js";
import { CYCLES_CYCLE_ID, CYCLES_PROJECT_ID, makeCycleRow } from "../testing.js";
import {
  CYCLE_TRANSFER_FAILED_TITLE,
  CYCLE_TRANSFERRED_LINE,
  CYCLE_TRANSFERRED_TITLE,
  cycleTransferListsKey,
  useTransferCycleWork,
} from "./cycleTransfer.js";
import { cycleKeys } from "./keys.js";
import { cyclesListQueryOptions } from "./queries.js";

const WS = "acme";
const TARGET_ID = "423e4567-e89b-12d3-a456-426614174003";

let queryClient: QueryClient;
let posts: unknown[];
let transferStatus: number;
/** Project list rows the stub serves; the transfer swaps in the post-move set. */
let listRows: unknown[];
let listRowsAfterTransfer: unknown[] | null;
let requests: string[];

function transport(url: string, init: { method?: string; body?: string }): Promise<unknown> {
  const method = init.method ?? "GET";
  requests.push(`${method} ${url}`);
  if (method === "POST" && url.endsWith("/transfer-issues/")) {
    posts.push(init.body === undefined ? null : JSON.parse(init.body));
    if (transferStatus === 200 && listRowsAfterTransfer !== null) listRows = listRowsAfterTransfer;
    const body = transferStatus === 200 ? { message: "moved" } : { detail: "boom" };
    return Promise.resolve({
      status: transferStatus,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(body),
    });
  }
  if (method === "GET" && url.split("?")[0]?.endsWith("/cycles/") === true) {
    return Promise.resolve({
      status: 200,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(listRows),
    });
  }
  return Promise.resolve({ status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify([]) });
}

function Host({ onDone }: { onDone: (ok: boolean) => void }): React.ReactElement {
  const { transfer, pending } = useTransferCycleWork({
    workspaceSlug: WS,
    projectId: CYCLES_PROJECT_ID,
    sourceCycleId: CYCLES_CYCLE_ID,
  });
  return (
    <button
      type="button"
      data-testid="move"
      data-pending={pending}
      onClick={() => {
        void transfer(TARGET_ID).then(onDone);
      }}
    >
      move
    </button>
  );
}

beforeEach(() => {
  queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  createAppClient("https://api.test", transport as never);
  posts = [];
  transferStatus = 200;
  listRows = [];
  listRowsAfterTransfer = null;
  requests = [];
  notify.notifyError.mockClear();
  notify.notifySuccess.mockClear();
});

afterEach(() => {
  resetAppClient();
});

describe("cycleTransferListsKey", () => {
  it("addresses one cycle entity's work-item lists", () => {
    expect(cycleTransferListsKey(WS, CYCLES_PROJECT_ID, CYCLES_CYCLE_ID)).toEqual([
      "ws",
      WS,
      "issues",
      "list",
      "cycle",
      CYCLES_PROJECT_ID,
      CYCLES_CYCLE_ID,
    ]);
  });
});

describe("useTransferCycleWork", () => {
  it("posts the destination, confirms, and refreshes both sides plus both lists", async () => {
    const done = vi.fn();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    render(
      <QueryClientProvider client={queryClient}>
        <Host onDone={done} />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByTestId("move"));
    await waitFor(() => expect(done).toHaveBeenCalledWith(true));
    expect(posts).toEqual([{ new_cycle_id: TARGET_ID }]);
    expect(notify.notifySuccess).toHaveBeenCalledWith(CYCLE_TRANSFERRED_TITLE, CYCLE_TRANSFERRED_LINE);
    expect(`${CYCLE_TRANSFERRED_TITLE} ${CYCLE_TRANSFERRED_LINE}`).toMatch(/transferred successfully/i);
    const keys = invalidate.mock.calls.map((call) => JSON.stringify(call[0]?.queryKey ?? null));
    expect(keys.some((key) => key.includes(CYCLES_CYCLE_ID))).toBe(true);
    expect(keys.some((key) => key.includes(TARGET_ID))).toBe(true);
    expect(keys.filter((key) => key.includes('"issues","list"'))).toHaveLength(2);
    invalidate.mockRestore();
  });

  it("reports a refusal and moves nothing", async () => {
    transferStatus = 400;
    const done = vi.fn();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    render(
      <QueryClientProvider client={queryClient}>
        <Host onDone={done} />
      </QueryClientProvider>
    );
    await act(async () => {
      fireEvent.click(screen.getByTestId("move"));
    });
    await waitFor(() => expect(done).toHaveBeenCalledWith(false));
    expect(notify.notifyError).toHaveBeenCalledWith(CYCLE_TRANSFER_FAILED_TITLE);
    expect(notify.notifySuccess).not.toHaveBeenCalled();
    expect(invalidate).not.toHaveBeenCalled();
    invalidate.mockRestore();
  });
});

/** Finished source holding one leftover; the row entry and ring read this record. */
function RefreshHost({ onDone }: { onDone: (ok: boolean) => void }): React.ReactElement {
  const { transfer } = useTransferCycleWork({
    workspaceSlug: WS,
    projectId: CYCLES_PROJECT_ID,
    sourceCycleId: CYCLES_CYCLE_ID,
  });
  const list = useQuery(cyclesListQueryOptions(getAppClient(), WS, CYCLES_PROJECT_ID));
  const source = (list.data ?? []).find((row) => row.id === CYCLES_CYCLE_ID) ?? null;
  return (
    <>
      <button
        type="button"
        data-testid="move"
        onClick={() => {
          void transfer(TARGET_ID).then(onDone);
        }}
      >
        move
      </button>
      <span data-testid="source-gate">{source === null ? "missing" : String(canTransferFromRow(source))}</span>
      <span data-testid="source-ring">{source === null ? "missing" : String(cycleProgressPercent(source))}</span>
    </>
  );
}

function cachedList(): CycleRow[] {
  const data = queryClient.getQueryData(cycleKeys.list(WS, CYCLES_PROJECT_ID));
  if (!Array.isArray(data)) throw new Error("expected the project list in the cache");
  return data as CycleRow[];
}

describe("transfer refreshes the source row without a reload (NEWFRONT-267)", () => {
  it("clears the source transfer entry and freezes its ring on the written snapshot", async () => {
    const sourcePre = makeCycleRow({
      status: "completed",
      total_issues: 3,
      completed_issues: 2,
      cancelled_issues: 0,
      progress_snapshot: {},
    });
    const targetPre = makeCycleRow({ id: TARGET_ID, name: "Target", status: "current", total_issues: 0 });
    listRows = [sourcePre, targetPre];
    listRowsAfterTransfer = [
      makeCycleRow({
        status: "completed",
        total_issues: 2,
        completed_issues: 2,
        cancelled_issues: 0,
        progress_snapshot: { total_issues: 3, completed_issues: 2, cancelled_issues: 0 },
      }),
      makeCycleRow({ id: TARGET_ID, name: "Target", status: "current", total_issues: 1 }),
    ];
    expect(canTransferFromRow(sourcePre)).toBe(true);

    const done = vi.fn();
    render(
      <QueryClientProvider client={queryClient}>
        <RefreshHost onDone={done} />
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("source-gate")).toHaveTextContent("true"));
    expect(screen.getByTestId("source-ring")).toHaveTextContent("67");

    fireEvent.click(screen.getByTestId("move"));
    await waitFor(() => expect(done).toHaveBeenCalledWith(true));
    await waitFor(() => expect(screen.getByTestId("source-gate")).toHaveTextContent("false"));

    // The list re-read lands after the move: no reload between the two.
    const postIndex = requests.findIndex((entry) => entry.startsWith("POST"));
    const listGets = requests.filter((entry) => entry.startsWith("GET") && entry.includes("/cycles/"));
    expect(postIndex).toBeGreaterThanOrEqual(0);
    expect(listGets.length).toBeGreaterThanOrEqual(2);
    expect(requests.lastIndexOf(listGets[listGets.length - 1] as string)).toBeGreaterThan(postIndex);

    const source = cachedList().find((row) => row.id === CYCLES_CYCLE_ID);
    const target = cachedList().find((row) => row.id === TARGET_ID);
    expect(source).toBeDefined();
    expect(target).toBeDefined();
    if (source === undefined || target === undefined) throw new Error("expected both cycles in the cache");
    // Row entry follows the live count (nothing left); the banner entry is
    // gone too now the server wrote the snapshot.
    expect(canTransferFromRow(source)).toBe(false);
    expect(canTransferFromBanner(source)).toBe(false);
    // The ring reads the frozen pre-move counts, so it cannot drift (§2).
    expect(Object.keys(source.progress_snapshot ?? {}).length).toBeGreaterThan(0);
    expect(cycleProgressPercent(source)).toBe(67);
    expect(screen.getByTestId("source-ring")).toHaveTextContent("67");
    // Both sides refresh: the target gained the moved item.
    expect(target.total_issues).toBe(1);
  });
});
