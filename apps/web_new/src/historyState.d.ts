// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Router history state (parity/specs/agents.md §9, AGT-053). The
// assistant landing hands its first message to the conversation it just
// started through one-shot navigation state; this augmentation is the
// single place that extra key is declared.

declare module "@tanstack/history" {
  interface HistoryState {
    /** First message a just-started conversation still has to deliver. */
    pendingMessage?: string;
  }
}
