// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Session state (Data layer page). Holds the sign-in lifecycle and the
// registry every Zustand store joins so sign-out clears all client state.
// Server data itself lives in TanStack Query; sign-out clears the whole
// cache plus every registered store.

import type { QueryClient } from "@tanstack/react-query";
import { create } from "zustand";

export type SessionStatus = "unknown" | "authenticated" | "expired" | "signed-out";

interface SessionState {
  status: SessionStatus;
  /** Where to return after sign-in, kept when the session expires. */
  returnUrl: string | null;
  markAuthenticated: () => void;
  markExpired: (returnUrl?: string) => void;
  markSignedOut: () => void;
}

function currentPath(): string | null {
  if (typeof globalThis.window === "undefined") return null;
  return `${globalThis.window.location.pathname}${globalThis.window.location.search}`;
}

export const useSessionStore = create<SessionState>()((set) => ({
  status: "unknown",
  returnUrl: null,
  markAuthenticated: () => set({ status: "authenticated", returnUrl: null }),
  markExpired: (returnUrl) => set({ status: "expired", returnUrl: returnUrl ?? currentPath() }),
  markSignedOut: () => set({ status: "signed-out", returnUrl: null }),
}));

/** A Zustand store reset, registered so sign-out clears everything. */
export type StoreReset = () => void;

const resetters = new Set<StoreReset>();

/**
 * Join the sign-out reset list. Each feature store calls this once at
 * module scope. Returns an unregister function for tests.
 */
export function registerStoreReset(reset: StoreReset): () => void {
  resetters.add(reset);
  return () => {
    resetters.delete(reset);
  };
}

export function resetRegisteredStores(): void {
  for (const reset of resetters) {
    reset();
  }
}

/** Sign out: clear every registered store, drop the query cache, update status. */
export function signOut(queryClient: QueryClient): void {
  resetRegisteredStores();
  queryClient.clear();
  useSessionStore.getState().markSignedOut();
}
