// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Stored transcript read (AGT-042): the page is keyed under the thread
// subtree, always fresh so a poll sees each answer, and asks for the
// caller's page size.
import { describe, expect, it, vi } from "vitest";

import { transcriptKey } from "./threadKeys.js";
import { assistantTranscriptQueryOptions } from "./transcript.js";

const ROW = {
  id: "22222222-2222-4222-8222-222222222222",
  role: "assistant",
  content: "answer",
  status: "completed",
  seq: 2,
  turn_id: null,
  payload: {},
  created_at: "2026-10-01T00:00:00Z",
  completed_at: "2026-10-01T00:00:00Z",
};

describe("assistantTranscriptQueryOptions", () => {
  it("keys the page under the thread subtree with no staleness", () => {
    const options = assistantTranscriptQueryOptions({} as never, "acme", "t1", 200);
    expect(options.queryKey).toEqual(transcriptKey("acme", "t1"));
    expect(options.queryKey).toEqual(["ws", "acme", "assistant", "thread", "t1", "messages"]);
    expect(options.staleTime).toBe(0);
  });

  it("reads the page through the contract with the caller's limit", async () => {
    const listAssistantMessages = vi.fn(async () => [ROW]);
    vi.doMock("@pidash/api-client", async (importOriginal) => ({
      ...(await importOriginal<typeof import("@pidash/api-client")>()),
      listAssistantMessages,
    }));
    vi.resetModules();
    const { assistantTranscriptQueryOptions: fresh } = await import("./transcript.js");
    const options = fresh({} as never, "acme", "t1", 50);
    await expect(options.queryFn?.({} as never)).resolves.toEqual([ROW]);
    expect(listAssistantMessages).toHaveBeenCalledWith({}, "acme", "t1", { limit: 50 });
    vi.doUnmock("@pidash/api-client");
  });
});
