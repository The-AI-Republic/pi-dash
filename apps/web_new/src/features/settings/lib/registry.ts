// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page seam for profile settings (parity/specs/profile.md §5, PROF-009/010).
// The shell owns the addresses, the grouped navigation and the frame; each
// tab slice registers its body here from its own chunk. A tab without a
// body renders the shell's quiet placeholder, so a route is never a dead
// end and a slice changes nothing in the shell. Every tab id is reserved:
// registering an unknown id is a programming error and throws.

import * as React from "react";

import { isProfileSettingsTab, type ProfileSettingsTabId } from "./tabs.js";

export type ProfileSettingsPageComponent = () => React.ReactNode;

interface PageRegistry {
  pages: Partial<Record<ProfileSettingsTabId, ProfileSettingsPageComponent>>;
}

const EMPTY: PageRegistry = { pages: {} };
let registry: PageRegistry = EMPTY;
const listeners = new Set<() => void>();

function emit(): void {
  for (const listener of listeners) listener();
}

function assertTab(tab: string): asserts tab is ProfileSettingsTabId {
  if (!isProfileSettingsTab(tab)) throw new Error(`Unknown profile settings tab: ${tab}`);
}

/**
 * Mount one tab's body. Returns an unregister callback; a slice that
 * remounts later replaces its own entry, and unmounting an entry that was
 * already superseded is a no-op.
 */
export function registerProfileSettingsPage(tab: ProfileSettingsTabId, page: ProfileSettingsPageComponent): () => void {
  assertTab(tab);
  registry = { ...registry, pages: { ...registry.pages, [tab]: page } };
  emit();
  return () => {
    if (registry.pages[tab] !== page) return;
    const pages = { ...registry.pages };
    delete pages[tab];
    registry = { ...registry, pages };
    emit();
  };
}

export function getProfileSettingsPage(tab: ProfileSettingsTabId): ProfileSettingsPageComponent | null {
  return registry.pages[tab] ?? null;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The registered page for a tab, re-rendering when a slice registers one. */
export function useProfileSettingsPage(tab: ProfileSettingsTabId): ProfileSettingsPageComponent | null {
  return React.useSyncExternalStore(
    subscribe,
    () => getProfileSettingsPage(tab),
    () => getProfileSettingsPage(tab)
  );
}

/** Clears every registration. Tests only; production pages unregister themselves. */
export function resetProfileSettingsPages(): void {
  registry = EMPTY;
  emit();
}
