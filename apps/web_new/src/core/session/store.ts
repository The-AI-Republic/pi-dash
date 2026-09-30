// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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
