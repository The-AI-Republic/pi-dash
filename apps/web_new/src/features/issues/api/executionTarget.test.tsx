// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ApiError } from "@pidash/api-client";
import { render, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import type { ExecutionTargetPatch } from "../../../shared/pickers/index.js";
import {
  EXECUTION_TARGET_FAILED_TITLE,
  executionTargetFieldError,
  useUpdateExecutionTarget,
} from "./executionTarget.js";
import { issueKeys } from "./keys.js";

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
  notifySuccess: vi.fn(),
}));

import { notifyError } from "../../../shared/shell/index.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE_ID = "323e4567-e89b-12d3-a456-426614174002";
const OTHER_ID = "423e4567-e89b-12d3-a456-426614174003";
const POD_ID = "523e4567-e89b-12d3-a456-426614174004";
const RUNNER_ID = "623e4567-e89b-12d3-a456-426614174005";

function row(id: string): Record<string, unknown> {
  return {
    id,
    name: `issue ${id.slice(0, 4)}`,
    assigned_pod_id: null,
    assigned_runner_id: null,
    agent_executor: null,
  };
}

interface StubOptions {
  patchStatus?: number;
  patchBody?: string;
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
      return {
        status: options.patchStatus ?? 204,
        url,
        headers: { get: () => null },
        text: async () => options.patchBody ?? "",
      };
    }
    return { status: 200, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
  return { calls };
}

describe("useUpdateExecutionTarget", () => {
  const listKey = [...issueKeys.all("acme"), "list", "probe"] as const;
  const detailKey = [...issueKeys.all("acme"), "detail", "probe"] as const;

  let handle: {
    isSaving: boolean;
    validationError: string | null;
    updateExecutionTarget: (input: ExecutionTargetPatch) => void;
  } | null = null;

  function Probe(): React.ReactElement {
    const { isSaving, validationError, updateExecutionTarget } = useUpdateExecutionTarget("acme", PROJECT_ID, ISSUE_ID);
    handle = { isSaving, validationError, updateExecutionTarget };
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

  it("sends one PATCH with the pod pin and keeps the optimistic values", async () => {
    const { patchCalls, queryClient } = setup();
    handle!.updateExecutionTarget({
      assigned_pod_id: POD_ID,
      assigned_runner_id: null,
      agent_executor: "local_runner",
    });
    await waitFor(() => expect(patchCalls()).toHaveLength(1));
    const patch = patchCalls()[0]!;
    expect(patch.url).toContain(`/projects/${PROJECT_ID}/issues/${ISSUE_ID}/`);
    expect(JSON.parse(patch.body)).toEqual({
      assigned_pod_id: POD_ID,
      assigned_runner_id: null,
      agent_executor: "local_runner",
    });
    await waitFor(() => expect(handle!.isSaving).toBe(false));
    expect(queryClient.getQueryData(detailKey)).toMatchObject({
      assigned_pod_id: POD_ID,
      assigned_runner_id: null,
      agent_executor: "local_runner",
    });
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("pins a runner inside its pod through the same single update", async () => {
    const { patchCalls, queryClient } = setup();
    handle!.updateExecutionTarget({
      assigned_pod_id: POD_ID,
      assigned_runner_id: RUNNER_ID,
      agent_executor: "local_runner",
    });
    await waitFor(() => expect(patchCalls()).toHaveLength(1));
    expect(JSON.parse(patchCalls()[0]!.body)).toEqual({
      assigned_pod_id: POD_ID,
      assigned_runner_id: RUNNER_ID,
      agent_executor: "local_runner",
    });
    await waitFor(() => expect(handle!.isSaving).toBe(false));
    expect(queryClient.getQueryData(detailKey)).toMatchObject({
      assigned_pod_id: POD_ID,
      assigned_runner_id: RUNNER_ID,
    });
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("pins an agent and clears the pod through the same single update", async () => {
    const { patchCalls } = setup();
    handle!.updateExecutionTarget({
      assigned_pod_id: null,
      assigned_runner_id: null,
      agent_executor: "cloud_agent",
    });
    await waitFor(() => expect(patchCalls()).toHaveLength(1));
    expect(JSON.parse(patchCalls()[0]!.body)).toEqual({
      assigned_pod_id: null,
      assigned_runner_id: null,
      agent_executor: "cloud_agent",
    });
  });

  it("applies optimistically while the PATCH is in flight", async () => {
    const hold: { release: () => void } = { release: () => {} };
    const { queryClient } = setup({ holdPatch: hold });
    handle!.updateExecutionTarget({
      assigned_pod_id: POD_ID,
      assigned_runner_id: null,
      agent_executor: "local_runner",
    });
    await waitFor(() =>
      expect(queryClient.getQueryData(detailKey)).toMatchObject({
        assigned_pod_id: POD_ID,
        agent_executor: "local_runner",
      })
    );
    expect(handle!.isSaving).toBe(true);
    hold.release();
    await waitFor(() => expect(handle!.isSaving).toBe(false));
  });

  it("rolls back and toasts when the PATCH fails", async () => {
    const { patchCalls, queryClient } = setup({ patchStatus: 500 });
    handle!.updateExecutionTarget({
      assigned_pod_id: POD_ID,
      assigned_runner_id: null,
      agent_executor: "local_runner",
    });
    await waitFor(() => expect(patchCalls()).toHaveLength(1));
    await waitFor(() => expect(handle!.isSaving).toBe(false));
    expect(handle!.validationError).toBe(null);
    expect(queryClient.getQueryData(detailKey)).toMatchObject({ assigned_pod_id: null, agent_executor: null });
    expect(queryClient.getQueryData(listKey)).toMatchObject({
      results: expect.arrayContaining([expect.objectContaining({ id: ISSUE_ID, assigned_pod_id: null })]),
    });
    expect(notifyError).toHaveBeenCalledWith(EXECUTION_TARGET_FAILED_TITLE, expect.any(String));
  });

  it("reports a validation rejection in place without toasting", async () => {
    const { patchCalls } = setup({
      patchStatus: 400,
      patchBody: JSON.stringify({
        assigned_runner_id: ["That runner belongs to another project."],
        code: "runner_wrong_project",
      }),
    });
    handle!.updateExecutionTarget({
      assigned_pod_id: POD_ID,
      assigned_runner_id: RUNNER_ID,
      agent_executor: "local_runner",
    });
    await waitFor(() => expect(patchCalls()).toHaveLength(1));
    await waitFor(() => expect(handle!.isSaving).toBe(false));
    expect(handle!.validationError).toBe("That runner belongs to another project.");
    expect(notifyError).not.toHaveBeenCalled();
  });
});

describe("executionTargetFieldError", () => {
  it("reads the first execution-target field message, runner first", () => {
    const error = new ApiError({
      status: 400,
      code: "http",
      message: "Bad request",
      fields: { assigned_pod_id: ["pod message"], assigned_runner_id: ["runner message"] },
    });
    expect(executionTargetFieldError(error)).toBe("runner message");
  });

  it("returns null for non-field failures", () => {
    expect(executionTargetFieldError(new Error("boom"))).toBe(null);
    expect(
      executionTargetFieldError(
        new ApiError({ status: 400, code: "http", message: "Bad request", fields: { name: ["taken"] } })
      )
    ).toBe(null);
    expect(
      executionTargetFieldError(
        new ApiError({
          status: 500,
          code: "http",
          message: "Server error",
          fields: { assigned_runner_id: ["runner message"] },
        })
      )
    ).toBe(null);
  });
});
