// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Stored transcript read (AGT-042/AGT-055): the read is keyed under the
// thread subtree, always fresh so a poll sees each answer, and walks the
// cursor one caller-sized page at a time until a short page ends it.
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

  it("reads one short page through the contract with the caller's limit", async () => {
    const listAssistantMessages = vi.fn(async () => [ROW]);
    vi.doMock("@pidash/api-client", async (importOriginal) => ({
      ...(await importOriginal<typeof import("@pidash/api-client")>()),
      listAssistantMessages,
    }));
    vi.resetModules();
    const { assistantTranscriptQueryOptions: fresh } = await import("./transcript.js");
    const options = fresh({} as never, "acme", "t1", 50);
    await expect(options.queryFn?.({} as never)).resolves.toEqual([ROW]);
    expect(listAssistantMessages).toHaveBeenCalledTimes(1);
    expect(listAssistantMessages).toHaveBeenCalledWith({}, "acme", "t1", { after: 0, limit: 50 });
    vi.doUnmock("@pidash/api-client");
  });

  it("walks the cursor while pages come back full", async () => {
    const first = { ...ROW, seq: 1 };
    const second = { ...ROW, id: "33333333-3333-4333-8333-333333333333", seq: 2 };
    const third = { ...ROW, id: "44444444-4444-4444-8444-444444444444", seq: 3 };
    const listAssistantMessages = vi.fn(
      async (_client: unknown, _slug: string, _thread: string, query: { after?: number }) =>
        query.after === 0 ? [first, second] : [third]
    );
    vi.doMock("@pidash/api-client", async (importOriginal) => ({
      ...(await importOriginal<typeof import("@pidash/api-client")>()),
      listAssistantMessages,
    }));
    vi.resetModules();
    const { assistantTranscriptQueryOptions: fresh } = await import("./transcript.js");
    const options = fresh({} as never, "acme", "t1", 2);
    await expect(options.queryFn?.({} as never)).resolves.toEqual([first, second, third]);
    expect(listAssistantMessages).toHaveBeenCalledTimes(2);
    expect(listAssistantMessages).toHaveBeenNthCalledWith(1, {}, "acme", "t1", { after: 0, limit: 2 });
    expect(listAssistantMessages).toHaveBeenNthCalledWith(2, {}, "acme", "t1", { after: 2, limit: 2 });
    vi.doUnmock("@pidash/api-client");
  });

  it("resolves an empty transcript without another page", async () => {
    const listAssistantMessages = vi.fn(async () => []);
    vi.doMock("@pidash/api-client", async (importOriginal) => ({
      ...(await importOriginal<typeof import("@pidash/api-client")>()),
      listAssistantMessages,
    }));
    vi.resetModules();
    const { assistantTranscriptQueryOptions: fresh } = await import("./transcript.js");
    const options = fresh({} as never, "acme", "t1", 2);
    await expect(options.queryFn?.({} as never)).resolves.toEqual([]);
    expect(listAssistantMessages).toHaveBeenCalledTimes(1);
    vi.doUnmock("@pidash/api-client");
  });
});
