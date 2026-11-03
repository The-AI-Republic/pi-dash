// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One page's ordered write channel (parity/specs/pages.md §5.1, PAGE-026,
// NEWFRONT-358). The record endpoint applies whichever PATCH arrives last,
// so two overlapping saves of one page's name can leave a prefix behind:
// the older, slower request lands after the newer one and wins. This queue
// gives each page one channel instead — at most one write is on the wire,
// and only the newest pending write waits behind it. Writes superseded
// before or during their turn are dropped, never sent late.
//
// The channels are held per page, not per screen: the detail route keeps
// its screen mounted while the operator walks the sidebar, so one screen
// renames many pages in turn. A name typed on the page being left must
// stay in that page's channel until it is sent, and a channel is released
// once it has nothing on the wire and nothing waiting (NEWFRONT-358).

/** What became of one enqueued write. */
export interface PageRenameOutcome {
  /** A newer write was enqueued before this one settled: it is stale. */
  superseded: boolean;
  /** The write refused; `superseded` is false whenever this is true. */
  failed: boolean;
  /** The refusal, for the caller to report or rethrow. */
  error?: unknown;
}

export type PageRenameSink = (outcome: PageRenameOutcome) => void;

/** One write waiting for the wire, and the caller to answer when it settles. */
interface PendingWrite<T> {
  payload: T;
  settle: PageRenameSink;
  superseded: boolean;
}

export interface PageRenameQueue<T> {
  /** Ask for a write; answers through `settle` once this payload is resolved. */
  enqueue: (payload: T, settle: PageRenameSink) => void;
}

/** Every page's channel, addressed by the payload's own page identity. */
export interface PageRenameChannels<T> {
  /** Ask for a write on `payload`'s page; answers through `settle`. */
  enqueue: (payload: T, settle: PageRenameSink) => void;
}

/**
 * Build one page's write channel over `write`. `write` is called with a
 * payload only once the previous call has settled, so the caller can never
 * have two writes of the same page in flight together.
 */
export function createPageRenameQueue<T>(
  write: (payload: T) => Promise<void>,
  onIdle?: () => void
): PageRenameQueue<T> {
  let inFlight: PendingWrite<T> | null = null;
  let waiting: PendingWrite<T> | null = null;

  const drain = async (): Promise<void> => {
    if (inFlight !== null || waiting === null) return;
    const item = waiting;
    waiting = null;
    inFlight = item;
    let error: unknown;
    let refused = false;
    try {
      await write(item.payload);
    } catch (failure) {
      error = failure;
      refused = true;
    }
    inFlight = null;
    // A newer write may have replaced this one while it sat on the wire;
    // the verdict is read now, when the answer is actually known.
    const superseded = item.superseded;
    item.settle({ superseded, failed: refused && !superseded, ...(refused ? { error } : {}) });
    void drain();
    // Settling may have started the next write; only a channel that now has
    // nothing on the wire and nothing waiting is spent.
    if (inFlight === null && waiting === null) onIdle?.();
  };

  return {
    enqueue: (payload, settle) => {
      if (inFlight !== null) inFlight.superseded = true;
      if (waiting !== null) {
        // The replaced write never reaches the wire; its caller learns now.
        try {
          waiting.settle({ superseded: true, failed: false });
        } finally {
          waiting = null;
        }
      }
      waiting = { payload, settle, superseded: false };
      void drain();
    },
  };
}

/**
 * Build one write channel per page. `keyOf` names the page a payload
 * belongs to; payloads for different pages never wait on each other, so
 * a pending name for one page is never dropped by a rename on another.
 * A channel is discarded once it is idle, so the map does not grow with
 * every page an operator visits. Only a channel empties itself, and it
 * does so before returning to its caller, so a payload arriving at that
 * moment joins the same channel instead of meeting a replaced entry.
 */
export function createPageRenameChannels<T>(
  keyOf: (payload: T) => string,
  write: (payload: T) => Promise<void>
): PageRenameChannels<T> {
  const channels = new Map<string, PageRenameQueue<T>>();
  return {
    enqueue: (payload, settle) => {
      const key = keyOf(payload);
      let channel = channels.get(key);
      if (channel === undefined) {
        const created = createPageRenameQueue<T>(write, () => channels.delete(key));
        channels.set(key, created);
        channel = created;
      }
      channel.enqueue(payload, settle);
    },
  };
}
