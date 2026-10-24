// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Transfer write (parity/specs/cycles.md §12, CYC-031): the move posts
// the destination, confirms with the pinned copy, refreshes both sides
// plus both work-item lists, and reports a refusal without moving.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const notify = vi.hoisted(() => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));
vi.mock("../../../shared/shell/toaster.js", () => ({
  notifyError: notify.notifyError,
  notifySuccess: notify.notifySuccess,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { CYCLES_CYCLE_ID, CYCLES_PROJECT_ID } from "../testing.js";
import {
  CYCLE_TRANSFER_FAILED_TITLE,
  CYCLE_TRANSFERRED_LINE,
  CYCLE_TRANSFERRED_TITLE,
  cycleTransferListsKey,
  useTransferCycleWork,
} from "./cycleTransfer.js";

const WS = "acme";
const TARGET_ID = "423e4567-e89b-12d3-a456-426614174003";

let queryClient: QueryClient;
let posts: unknown[];
let transferStatus: number;

function transport(url: string, init: { method?: string; body?: string }): Promise<unknown> {
  const method = init.method ?? "GET";
  if (method === "POST" && url.endsWith("/transfer-issues/")) {
    posts.push(init.body === undefined ? null : JSON.parse(init.body));
    const body = transferStatus === 200 ? { message: "moved" } : { detail: "boom" };
    return Promise.resolve({
      status: transferStatus,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(body),
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
