// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The per-page rename channel (parity/specs/pages.md §5.1, NEWFRONT-358):
// one write on the wire at a time, the newest name behind it, and stale
// names dropped rather than sent late.

import { describe, expect, it, vi } from "vitest";

import { createPageRenameQueue, type PageRenameOutcome } from "./pageRenameQueue.js";

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
    let release: (() => void) | null = null;
    const queue = createPageRenameQueue<string>((payload) => {
      seen.push(payload);
      return new Promise<void>((resolve) => {
        release = resolve;
      });
    });
    queue.enqueue("first", outcomes().settle);
    queue.enqueue("second", outcomes().settle);
    expect(seen).toEqual(["first"]);
    release?.();
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
