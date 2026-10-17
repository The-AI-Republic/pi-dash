// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { issueKeys } from "./keys.js";
import { ISSUE_DATES_FAILED_TITLE, useUpdateIssueDates, withIssueDates, type IssueDatesInput } from "./dates.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError } from "../../../shared/shell/index.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE_ID = "323e4567-e89b-12d3-a456-426614174002";
const OTHER_ID = "423e4567-e89b-12d3-a456-426614174003";
const DATES: IssueDatesInput = { startDate: "2026-10-10", endDate: "2026-10-20" };

function row(id: string): Record<string, unknown> {
  return { id, name: `issue ${id.slice(0, 4)}`, start_date: null, target_date: null };
}

describe("withIssueDates", () => {
  it("patches the matching row in a bare list and skips the rest", () => {
    expect(withIssueDates([row(ISSUE_ID), row(OTHER_ID), "stray"], ISSUE_ID, DATES)).toEqual([
      { ...row(ISSUE_ID), start_date: "2026-10-10", target_date: "2026-10-20" },
      row(OTHER_ID),
      "stray",
    ]);
  });

  it("patches a detail object only when the id matches", () => {
    expect(withIssueDates(row(ISSUE_ID), ISSUE_ID, DATES)).toEqual({
      ...row(ISSUE_ID),
      start_date: "2026-10-10",
      target_date: "2026-10-20",
    });
    expect(withIssueDates(row(OTHER_ID), ISSUE_ID, DATES)).toEqual(row(OTHER_ID));
    expect(withIssueDates(42, ISSUE_ID, DATES)).toBe(42);
    expect(withIssueDates(null, ISSUE_ID, DATES)).toBe(null);
  });

  it("patches rows inside a flat list envelope", () => {
    expect(withIssueDates({ count: 2, results: [row(ISSUE_ID), row(OTHER_ID)] }, ISSUE_ID, DATES)).toEqual({
      count: 2,
      results: [{ ...row(ISSUE_ID), start_date: "2026-10-10", target_date: "2026-10-20" }, row(OTHER_ID)],
    });
  });

  it("patches rows inside grouped buckets and keeps stray entries", () => {
    expect(
      withIssueDates(
        {
          count: 1,
          results: {
            todo: { results: [row(ISSUE_ID)], total_results: 1 },
            empty: { total_results: 0 },
            stray: 7,
          },
        },
        ISSUE_ID,
        DATES
      )
    ).toEqual({
      count: 1,
      results: {
        todo: {
          results: [{ ...row(ISSUE_ID), start_date: "2026-10-10", target_date: "2026-10-20" }],
          total_results: 1,
        },
        empty: { total_results: 0 },
        stray: 7,
      },
    });
  });

  it("passes unknown records through untouched", () => {
    expect(withIssueDates({ filters: {} }, ISSUE_ID, DATES)).toEqual({ filters: {} });
  });
});

interface StubOptions {
  patchStatus?: number;
  holdPatch?: { release: () => void } | null;
}

function stubClient(options: StubOptions = {}) {
  const calls: Array<{ url: string; method: string; body: string }> = [];
  createAppClient("https://api.test", (async (url: string, init: { method?: string; body?: string }) => {
    calls.push({ url, method: init.method ?? "GET", body: init.body ?? "" });
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    if (init.method === "PATCH") {
      if (options.holdPatch) await new Promise<void>((resolve) => (options.holdPatch!.release = resolve));
      return { status: options.patchStatus ?? 204, url, headers: { get: () => null }, text: async () => "" };
    }
    return { status: 200, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
  return { calls };
}

describe("useUpdateIssueDates", () => {
  const listKey = [...issueKeys.all("acme"), "list", "probe"] as const;
  const detailKey = [...issueKeys.all("acme"), "detail", "probe"] as const;

  let handle: { isSaving: boolean; updateIssueDates: (input: IssueDatesInput) => void } | null = null;

  function Probe(): React.ReactElement {
    const { isSaving, updateIssueDates } = useUpdateIssueDates("acme", PROJECT_ID, ISSUE_ID);
    handle = { isSaving, updateIssueDates };
    return <span data-saving={isSaving ? "true" : "false"} />;
  }

  function setup(options: StubOptions = {}) {
    handle = null;
    vi.mocked(notifyError).mockClear();
    const { calls } = stubClient(options);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(listKey, { count: 2, results: [row(ISSUE_ID), row(OTHER_ID)] });
    queryClient.setQueryData(detailKey, row(ISSUE_ID));
    render(
      <QueryClientProvider client={queryClient}>
        <Probe />
      </QueryClientProvider>
    );
    if (handle === null) throw new Error("probe did not render");
    const patchCalls = () => calls.filter((call) => call.method === "PATCH");
    return { calls, patchCalls, queryClient };
  }

  afterEach(() => {
    resetAppClient();
  });

  it("sends one PATCH with both dates and keeps the optimistic values", async () => {
    const { patchCalls, queryClient } = setup();
    handle!.updateIssueDates(DATES);
    await waitFor(() => expect(patchCalls()).toHaveLength(1));
    const patch = patchCalls()[0]!;
    expect(patch.url).toContain(`/projects/${PROJECT_ID}/issues/${ISSUE_ID}/`);
    expect(JSON.parse(patch.body)).toEqual({ start_date: "2026-10-10", target_date: "2026-10-20" });
    await waitFor(() => expect(handle!.isSaving).toBe(false));
    expect(queryClient.getQueryData(detailKey)).toMatchObject({ start_date: "2026-10-10", target_date: "2026-10-20" });
    expect(queryClient.getQueryData(listKey)).toMatchObject({
      results: expect.arrayContaining([expect.objectContaining({ id: ISSUE_ID, start_date: "2026-10-10" })]),
    });
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("clears both ends through the same single update", async () => {
    const { patchCalls } = setup();
    handle!.updateIssueDates({ startDate: null, endDate: null });
    await waitFor(() => expect(patchCalls()).toHaveLength(1));
    expect(JSON.parse(patchCalls()[0]!.body)).toEqual({ start_date: null, target_date: null });
  });

  it("applies optimistically while the PATCH is in flight", async () => {
    const hold: { release: () => void } = { release: () => {} };
    const { queryClient } = setup({ holdPatch: hold });
    handle!.updateIssueDates(DATES);
    await waitFor(() =>
      expect(queryClient.getQueryData(detailKey)).toMatchObject({
        start_date: "2026-10-10",
        target_date: "2026-10-20",
      })
    );
    expect(handle!.isSaving).toBe(true);
    hold.release();
    await waitFor(() => expect(handle!.isSaving).toBe(false));
  });

  it("rolls back and toasts when the PATCH fails", async () => {
    const { patchCalls, queryClient } = setup({ patchStatus: 500 });
    handle!.updateIssueDates(DATES);
    await waitFor(() => expect(patchCalls()).toHaveLength(1));
    await waitFor(() => expect(handle!.isSaving).toBe(false));
    expect(queryClient.getQueryData(detailKey)).toMatchObject({ start_date: null, target_date: null });
    expect(queryClient.getQueryData(listKey)).toMatchObject({
      results: expect.arrayContaining([expect.objectContaining({ id: ISSUE_ID, start_date: null })]),
    });
    expect(notifyError).toHaveBeenCalledWith(ISSUE_DATES_FAILED_TITLE, expect.any(String));
  });
});
