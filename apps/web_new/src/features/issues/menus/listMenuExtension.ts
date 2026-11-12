// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Edition seam for the whole-list ellipsis (ISS-067). OSS registers
// nothing; the cloud edition adds its own entries on boot. Entries append
// after the built-in open-in-new-tab and copy-link pair, never replace
// them, so every edition keeps the free actions the parity scenario pins.

export interface ListMenuExtra {
  /** Stable key for React and for callers that remove an entry. */
  key: string;
  label: string;
  onSelect: () => void;
}

let extras: ListMenuExtra[] = [];

/** Add edition entries; returns the unregister function. */
export function registerListMenuExtension(entries: readonly ListMenuExtra[]): () => void {
  const previous = extras;
  extras = [...extras, ...entries];
  return () => {
    extras = previous;
  };
}

/** Entries currently appended to every whole-list menu. */
export function listMenuExtraEntries(): readonly ListMenuExtra[] {
  return extras;
}

/** Test helper: drop every registered entry. */
export function resetListMenuExtension(): void {
  extras = [];
}
