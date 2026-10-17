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
import { ESTIMATE_POINT_FAILED_TITLE, useUpdateEstimatePoint } from "./estimates.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError } from "../../../shared/shell/index.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE_ID = "323e4567-e89b-12d3-a456-426614174002";
const OTHER_ID = "423e4567-e89b-12d3-a456-426614174003";
const POINT_ID = "523e4567-e89b-12d3-a456-426614174004";

function row(id: string): Record<string, unknown> {
  return { id, name: `issue ${id.slice(0, 4)}`, estimate_point: null };
}

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

describe("useUpdateEstimatePoint", () => {
  const listKey = [...issueKeys.all("acme"), "list", "probe"] as const;
  const detailKey = [...issueKeys.all("acme"), "detail", "probe"] as const;

  let handle: { isSaving: boolean; updateEstimatePoint: (pointId: string | null) => void } | null = null;

  function Probe(): React.ReactElement {
    const { isSaving, updateEstimatePoint } = useUpdateEstimatePoint("acme", PROJECT_ID, ISSUE_ID);
    handle = { isSaving, updateEstimatePoint };
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
    return { patchCalls, queryClient };
  }

  afterEach(() => {
    resetAppClient();
  });

  it("sends one PATCH with the point and keeps the optimistic values", async () => {
    const { patchCalls, queryClient } = setup();
    handle!.updateEstimatePoint(POINT_ID);
    await waitFor(() => expect(patchCalls()).toHaveLength(1));
    const patch = patchCalls()[0]!;
    expect(patch.url).toContain(`/projects/${PROJECT_ID}/issues/${ISSUE_ID}/`);
    expect(JSON.parse(patch.body)).toEqual({ estimate_point: POINT_ID });
    await waitFor(() => expect(handle!.isSaving).toBe(false));
    expect(queryClient.getQueryData(detailKey)).toMatchObject({ estimate_point: POINT_ID });
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("clears the point through the same single update", async () => {
    const { patchCalls } = setup();
    handle!.updateEstimatePoint(null);
    await waitFor(() => expect(patchCalls()).toHaveLength(1));
    expect(JSON.parse(patchCalls()[0]!.body)).toEqual({ estimate_point: null });
  });

  it("applies optimistically while the PATCH is in flight", async () => {
    const hold: { release: () => void } = { release: () => {} };
    const { queryClient } = setup({ holdPatch: hold });
    handle!.updateEstimatePoint(POINT_ID);
    await waitFor(() => expect(queryClient.getQueryData(detailKey)).toMatchObject({ estimate_point: POINT_ID }));
    expect(handle!.isSaving).toBe(true);
    hold.release();
    await waitFor(() => expect(handle!.isSaving).toBe(false));
  });

  it("rolls back and toasts when the PATCH fails", async () => {
    const { patchCalls, queryClient } = setup({ patchStatus: 500 });
    handle!.updateEstimatePoint(POINT_ID);
    await waitFor(() => expect(patchCalls()).toHaveLength(1));
    await waitFor(() => expect(handle!.isSaving).toBe(false));
    expect(queryClient.getQueryData(detailKey)).toMatchObject({ estimate_point: null });
    expect(queryClient.getQueryData(listKey)).toMatchObject({
      results: expect.arrayContaining([expect.objectContaining({ id: ISSUE_ID, estimate_point: null })]),
    });
    expect(notifyError).toHaveBeenCalledWith(ESTIMATE_POINT_FAILED_TITLE, expect.any(String));
  });
});
