// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake request creation (parity/specs/intake.md INT-013): the write
// reaches the intake-create endpoint with the dialog's payload, and a
// success re-reads every tab list of the project.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import type { IntakeIssueCreateInput, IntakeIssueDetail } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { createIntakeIssue: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { intakeKeys } from "./keys.js";
import { useCreateIntakeRequest } from "./create.js";

const WS = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";

function created(input: IntakeIssueCreateInput): IntakeIssueDetail {
  return {
    id: "aaaaaaaa-aaaa-4aaa-8aaa-000000000001",
    status: -2,
    duplicate_to: null,
    snoozed_till: null,
    duplicate_issue_detail: null,
    source: "IN_APP",
    issue: {
      id: ISSUE,
      name: input.issue.name,
      description_html: input.issue.description_html ?? "",
      priority: (input.issue.priority ?? "none") as IntakeIssueDetail["issue"]["priority"],
      state_id: input.issue.state_id ?? "",
      sequence_id: 7,
      project_id: PROJECT,
      parent_id: null,
      label_ids: input.issue.label_ids ?? [],
      assignee_ids: input.issue.assignee_ids ?? [],
      target_date: input.issue.target_date ?? null,
      start_date: null,
      estimate_point: null,
      sort_order: 1,
      completed_at: null,
      created_at: "2026-10-23T00:00:00Z",
      updated_at: "2026-10-23T00:00:00Z",
      created_by: null,
      updated_by: null,
      is_draft: false,
      archived_at: null,
      complexity_score: 0,
      agent_executor: null,
      is_synced: false,
      is_subscribed: false,
      agent_ticker: null,
      agent_status: null,
      relations_summary: { blocked_by: [], blocking: [] },
      has_open_blockers: false,
    },
  };
}

beforeEach(() => {
  contractMocks.createIntakeIssue.mockReset();
  createAppClient("https://api.test", (async () => {
    throw new Error("unexpected request");
  }) as never);
});

afterEach(() => {
  resetAppClient();
});

describe("useCreateIntakeRequest (INT-013)", () => {
  it("posts the dialog payload and re-reads the project's tab lists", async () => {
    const input: IntakeIssueCreateInput = {
      source: "IN_APP",
      issue: { name: "Triage me", state_id: "" },
    };
    contractMocks.createIntakeIssue.mockResolvedValue(created(input));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useCreateIntakeRequest(WS, PROJECT), { wrapper });

    let answer: IntakeIssueDetail | undefined;
    await act(async () => {
      answer = await result.current.mutateAsync(input);
    });

    expect(contractMocks.createIntakeIssue).toHaveBeenCalledTimes(1);
    expect(contractMocks.createIntakeIssue).toHaveBeenCalledWith(expect.anything(), WS, PROJECT, input);
    expect(answer?.issue.id).toBe(ISSUE);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: intakeKeys.lists(WS, PROJECT) });
  });
});
