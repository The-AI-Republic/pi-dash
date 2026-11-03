// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The per-page rename channel (parity/specs/pages.md §5.1, NEWFRONT-358):
// one write on the wire at a time, the newest name behind it, and stale
// names dropped rather than sent late.

import { describe, expect, it, vi } from "vitest";

import { createPageRenameChannels, createPageRenameQueue, type PageRenameOutcome } from "./pageRenameQueue.js";

interface Gate {
  resolve: () => void;
  reject: (error: unknown) => void;
  name: string;
}

interface GatedWrites {
  write: (name: string) => Promise<void>;
  calls: Gate[];
  settle: (index: number) => void;
  refuse: (index: number, error: unknown) => void;
}

/** A write sink whose calls the test releases one at a time. */
function gatedWrites(): GatedWrites {
  const calls: Gate[] = [];
  const write = (name: string): Promise<void> =>
    new Promise<void>((resolve, reject) => {
      calls.push({ name, resolve, reject });
    });
  return {
    write,
    calls,
    settle: (index) => {
      calls[index]?.resolve();
    },
    refuse: (index, error) => {
      calls[index]?.reject(error);
    },
  };
}

function outcomes(): { seen: PageRenameOutcome[]; settle: (outcome: PageRenameOutcome) => void } {
  const seen: PageRenameOutcome[] = [];
  return { seen, settle: (outcome) => seen.push(outcome) };
}

describe("createPageRenameQueue", () => {
  it("sends one name at a time and answers the caller on settle", async () => {
    const sink = gatedWrites();
    const queue = createPageRenameQueue(sink.write);
    const first = outcomes();
    queue.enqueue("Alpha", first.settle);
    expect(sink.calls.map((call) => call.name)).toEqual(["Alpha"]);
    sink.settle(0);
    await vi.waitFor(() => expect(first.seen).toEqual([{ superseded: false, failed: false }]));
  });

  it("holds the newest name behind the one on the wire", async () => {
    const sink = gatedWrites();
    const queue = createPageRenameQueue(sink.write);
    const first = outcomes();
    const second = outcomes();
    queue.enqueue("A", first.settle);
    queue.enqueue("AB", second.settle);
    expect(sink.calls).toHaveLength(1);
    sink.settle(0);
    await vi.waitFor(() => expect(sink.calls.map((call) => call.name)).toEqual(["A", "AB"]));
    expect(sink.calls).toHaveLength(2);
    sink.settle(1);
    await vi.waitFor(() => expect(second.seen).toEqual([{ superseded: false, failed: false }]));
  });

  it("replaces a waiting name, dropping it unsent, so the last keystroke is the last write", async () => {
    const sink = gatedWrites();
    const queue = createPageRenameQueue(sink.write);
    const first = outcomes();
    const dropped = outcomes();
    const last = outcomes();
    queue.enqueue("A", first.settle);
    queue.enqueue("AB", dropped.settle);
    queue.enqueue("ABC", last.settle);
    expect(dropped.seen).toEqual([{ superseded: true, failed: false }]);
    sink.settle(0);
    await vi.waitFor(() => expect(sink.calls.map((call) => call.name)).toEqual(["A", "ABC"]));
    // The dropped name never reaches the wire.
    expect(sink.calls).toHaveLength(2);
    // The name on the wire when a newer one arrives is answered as superseded.
    await vi.waitFor(() => expect(first.seen).toEqual([{ superseded: true, failed: false }]));
    sink.settle(1);
    await vi.waitFor(() => expect(last.seen).toEqual([{ superseded: false, failed: false }]));
  });

  it("treats the in-flight name as stale once a newer one queues, even before it settles", () => {
    const sink = gatedWrites();
    const queue = createPageRenameQueue(sink.write);
    const first = outcomes();
    queue.enqueue("A", first.settle);
    queue.enqueue("AB", outcomes().settle);
    expect(first.seen).toEqual([]);
    sink.settle(0);
    return vi.waitFor(() => expect(first.seen).toEqual([{ superseded: true, failed: false }]));
  });

  it("hands each write the payload it was enqueued with, not a later one", async () => {
    const seen: string[] = [];
    const opened: { release?: () => void } = {};
    const queue = createPageRenameQueue<string>((payload) => {
      seen.push(payload);
      return new Promise<void>((resolve) => {
        opened.release = resolve;
      });
    });
    queue.enqueue("first", outcomes().settle);
    queue.enqueue("second", outcomes().settle);
    expect(seen).toEqual(["first"]);
    opened.release?.();
    await vi.waitFor(() => expect(seen).toEqual(["first", "second"]));
  });

  it("reports a refusal once the wire frees, without wedging the queue", async () => {
    const sink = gatedWrites();
    const queue = createPageRenameQueue(sink.write);
    const first = outcomes();
    const second = outcomes();
    const failure = new Error("boom");
    queue.enqueue("A", first.settle);
    sink.refuse(0, failure);
    await vi.waitFor(() => expect(first.seen).toEqual([{ superseded: false, failed: true, error: failure }]));
    queue.enqueue("AB", second.settle);
    expect(sink.calls.map((call) => call.name)).toEqual(["A", "AB"]);
    sink.settle(1);
    await vi.waitFor(() => expect(second.seen).toEqual([{ superseded: false, failed: false }]));
  });

  it("does not report a superseded refusal as a failure", async () => {
    const sink = gatedWrites();
    const queue = createPageRenameQueue(sink.write);
    const first = outcomes();
    queue.enqueue("A", first.settle);
    queue.enqueue("AB", outcomes().settle);
    sink.refuse(0, new Error("too late"));
    await vi.waitFor(() => expect(first.seen).toEqual([{ superseded: true, failed: false, error: expect.anything() }]));
  });
});

interface PageWrite {
  pageId: string;
  name: string;
}

describe("createPageRenameChannels", () => {
  /** A per-page write sink whose calls the test releases one at a time. */
  function pagedWrites(): {
    write: (payload: PageWrite) => Promise<void>;
    sent: PageWrite[];
    release: (index: number) => void;
  } {
    const sent: PageWrite[] = [];
    const open: Array<() => void> = [];
    return {
      sent,
      release: (index) => open[index]?.(),
      write: (payload) => {
        sent.push(payload);
        return new Promise<void>((resolve) => open.push(resolve));
      },
    };
  }

  it("keeps one write at a time within a page", async () => {
    const sink = pagedWrites();
    const channels = createPageRenameChannels<PageWrite>((payload) => payload.pageId, sink.write);
    const first = outcomes();
    channels.enqueue({ pageId: "A", name: "A1" }, first.settle);
    channels.enqueue({ pageId: "A", name: "A2" }, outcomes().settle);
    expect(sink.sent.map((payload) => payload.name)).toEqual(["A1"]);
    sink.release(0);
    await vi.waitFor(() => expect(sink.sent.map((payload) => payload.name)).toEqual(["A1", "A2"]));
  });

  it("does not let a rename on one page supersede a waiting name on another", async () => {
    const sink = pagedWrites();
    const channels = createPageRenameChannels<PageWrite>((payload) => payload.pageId, sink.write);
    const first = outcomes();
    const left = outcomes();
    const next = outcomes();
    channels.enqueue({ pageId: "A", name: "A1" }, first.settle);
    channels.enqueue({ pageId: "A", name: "A final" }, left.settle);
    channels.enqueue({ pageId: "B", name: "B1" }, next.settle);
    // B writes on its own channel, beside A's one in-flight write.
    expect(sink.sent.map((payload) => payload.name)).toEqual(["A1", "B1"]);
    expect(left.seen).toEqual([]);
    sink.release(0);
    await vi.waitFor(() => expect(sink.sent.map((payload) => payload.name)).toEqual(["A1", "B1", "A final"]));
    expect(left.seen).toEqual([]);
    sink.release(1);
    sink.release(2);
    await vi.waitFor(() => expect(left.seen).toEqual([{ superseded: false, failed: false }]));
    await vi.waitFor(() => expect(next.seen).toEqual([{ superseded: false, failed: false }]));
  });

  it("releases a channel once it is idle so the next write starts fresh", async () => {
    const sink = pagedWrites();
    const channels = createPageRenameChannels<PageWrite>((payload) => payload.pageId, sink.write);
    const first = outcomes();
    channels.enqueue({ pageId: "A", name: "A1" }, first.settle);
    sink.release(0);
    await vi.waitFor(() => expect(first.seen).toEqual([{ superseded: false, failed: false }]));
    const second = outcomes();
    channels.enqueue({ pageId: "A", name: "A2" }, second.settle);
    expect(sink.sent.map((payload) => payload.name)).toEqual(["A1", "A2"]);
    sink.release(1);
    await vi.waitFor(() => expect(second.seen).toEqual([{ superseded: false, failed: false }]));
  });

  it("keeps a channel open while a newer name is still waiting", async () => {
    const sink = pagedWrites();
    const channels = createPageRenameChannels<PageWrite>((payload) => payload.pageId, sink.write);
    const last = outcomes();
    channels.enqueue({ pageId: "A", name: "A1" }, outcomes().settle);
    channels.enqueue({ pageId: "A", name: "A2" }, last.settle);
    sink.release(0);
    await vi.waitFor(() => expect(sink.sent.map((payload) => payload.name)).toEqual(["A1", "A2"]));
    sink.release(1);
    await vi.waitFor(() => expect(last.seen).toEqual([{ superseded: false, failed: false }]));
  });
});

interface PageWrite {
  pageId: string;
  name: string;
}
