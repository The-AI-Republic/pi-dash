// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Appearance theme provider (shell spec, SHELL-095). Owns the root
// theme mark for the session: the stored choice applies at boot, the
// server profile syncs once per user, the OS is tracked while the
// system choice holds, and every change re-skins immediately (no
// reload). Failures toast and undo the pick; the next boot re-syncs
// from the server. The error notifier arrives by prop so this
// core module never imports the shared toaster.
//
// A pick applies and stores locally at once, then persists to the profile.
// A failed write (NEWFRONT-434 target) restores the previous choice AND
// the applied document theme together, so what renders always matches
// what the picker shows. Re-picking custom with a saved palette re-applies
// the saved colors immediately.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { updateProfile } from "@pidash/api-client";
import { watchSystemTheme } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../api/client.js";
import { sessionKeys, useProfile } from "../session/index.js";
import { applyResolvedTheme, prefersDarkScheme } from "./apply.js";
import {
  buildThemeBag,
  clearStoredPalette,
  readStoredPalette,
  readStoredTheme,
  readThemeBag,
  resolveScheme,
  writeStoredPalette,
  writeStoredTheme,
  type CustomPalette,
  type ResolvedScheme,
  type ThemeValue,
} from "./theme.js";

export interface ThemeContextValue {
  /** The stored choice (what the pickers show selected). */
  value: ThemeValue;
  /** The resolved document scheme (what data-theme carries). */
  resolved: ResolvedScheme;
  /** The custom palette when the choice is custom with saved colors. */
  custom: CustomPalette | undefined;
  /** True while the profile write is in flight. */
  isSaving: boolean;
  /**
   * Choose a theme: applies and stores locally at once, then persists
   * to the profile. Resolves true once the write lands and false when
   * it fails (the toaster reports the failure and the last accepted
   * choice comes back with the document theme). Callers that must act
   * on the stored result, such as reloading after a custom save, await
   * this instead of racing the write.
   */
  setTheme: (value: ThemeValue, custom?: CustomPalette | undefined) => Promise<boolean>;
}

const ThemeContext = React.createContext<ThemeContextValue | null>(null);

/** Read the appearance theme; must render under ThemeProvider. */
export function useTheme(): ThemeContextValue {
  const context = React.useContext(ThemeContext);
  if (context === null) throw new Error("useTheme must render under ThemeProvider");
  return context;
}

/** Read a document theme mark defensively (anything unknown is light). */
export function readDocumentScheme(marked: string | null): ResolvedScheme {
  return marked === "dark" || marked === "dark-contrast" || marked === "light-contrast" || marked === "custom"
    ? marked
    : "light";
}

/**
 * Read the resolved scheme, falling back to the document mark outside
 * the provider (static shells and tests render without one). The
 * fallback reads once; only provided consumers track live switches.
 */
export function useResolvedScheme(): { scheme: ResolvedScheme; customDark: boolean } {
  const context = React.useContext(ThemeContext);
  if (context !== null) return { scheme: context.resolved, customDark: context.custom?.darkPalette ?? false };
  return { scheme: readDocumentScheme(document.documentElement.getAttribute("data-theme")), customDark: false };
}

function rootElement(): HTMLElement {
  return document.documentElement;
}

/** Apply one choice to the document root and return its resolved scheme. */
function applyChoice(value: ThemeValue, custom: CustomPalette | undefined): ResolvedScheme {
  const scheme = resolveScheme(value, prefersDarkScheme());
  applyResolvedTheme(scheme, value === "custom" ? custom : undefined, rootElement());
  return scheme;
}

/** The stored picker choice (what the switcher shows selected). */
interface Choice {
  value: ThemeValue;
  custom: CustomPalette | undefined;
}

/**
 * What the document currently renders. Undoing a failed pick needs both
 * halves: the scheme AND the custom palette the root carries, so the
 * restored app paints exactly what the restored switcher names.
 */
interface AppliedTheme {
  scheme: ResolvedScheme;
  custom: CustomPalette | undefined;
}

export interface ThemeProviderProps {
  children: React.ReactNode;
  /** Error surface for failed profile writes (the shared toaster). */
  notifyError: (title: string, description?: string) => void;
}

export function ThemeProvider({ children, notifyError }: ThemeProviderProps): React.ReactElement {
  const queryClient = useQueryClient();
  const profileQuery = useProfile();
  const [choice, setChoice] = React.useState<Choice>(() => {
    const value = readStoredTheme();
    return { value, custom: value === "custom" ? readStoredPalette() : undefined };
  });
  const [applied, setApplied] = React.useState<AppliedTheme>(() => {
    const custom = choice.value === "custom" ? choice.custom : undefined;
    return { scheme: resolveScheme(choice.value, prefersDarkScheme()), custom };
  });
  const resolved = applied.scheme;
  const syncedUser = React.useRef<string | null>(null);
  // The last choice the server accepted (or that came from it, or the OS
  // resolved). A failed write restores this one, switcher and document
  // together. It is set on success only, never on the optimistic paint.
  const committed = React.useRef<{ choice: Choice; scheme: ResolvedScheme } | null>(null);
  if (committed.current === null) {
    committed.current = { choice, scheme: resolveScheme(choice.value, prefersDarkScheme()) };
  }

  const save = useMutation({
    mutationFn: async (bag: Record<string, unknown>) => updateProfile(getAppClient(), { theme: bag }),
    onSuccess: (profile) => {
      queryClient.setQueryData(sessionKeys.profile, profile);
    },
    onError: (error: Error) => {
      notifyError("Couldn't update the theme", error.message);
    },
  });

  /** Apply a choice everywhere at once: state, document and local copy. */
  const applyState = React.useCallback((next: Choice): ResolvedScheme => {
    const custom = next.value === "custom" ? next.custom : undefined;
    const scheme = applyChoice(next.value, custom);
    writeStoredTheme(next.value);
    if (custom !== undefined) writeStoredPalette(custom);
    setChoice({ value: next.value, custom });
    setApplied({ scheme, custom });
    return scheme;
  }, []);

  const cachedBag = profileQuery.data?.theme;
  const setTheme = React.useCallback(
    (value: ThemeValue, custom: CustomPalette | undefined = undefined): Promise<boolean> => {
      // Picking custom without new colors re-applies the saved palette;
      // a member with no saved palette keeps the pick bare, so the server
      // bag gains only the theme name until the designer is saved.
      const saved = value === "custom" ? (custom ?? readThemeBag(cachedBag).custom) : undefined;
      const next: Choice = { value, custom: saved };
      const scheme = applyState(next);
      return save
        .mutateAsync(buildThemeBag(value, saved, cachedBag ?? {}))
        .then(() => {
          committed.current = { choice: next, scheme };
          return true;
        })
        .catch(() => {
          // The toaster already reported this write; here the failed pick
          // is undone completely so the app keeps matching the switcher
          // (NEWFRONT-434 target).
          const previous = committed.current!;
          applyResolvedTheme(previous.scheme, previous.choice.custom, rootElement());
          writeStoredTheme(previous.choice.value);
          // Undo the local palette too: a rejected custom save must not
          // paint its colors on the next boot (NEWFRONT-434 target).
          if (previous.choice.custom === undefined) clearStoredPalette();
          else writeStoredPalette(previous.choice.custom);
          setChoice(previous.choice);
          setApplied({ scheme: previous.scheme, custom: previous.choice.custom });
          return false;
        });
    },
    [save, cachedBag, applyState]
  );

  // Boot mark plus OS tracking while the system choice holds. The OS
  // watcher re-applies on flips (and updates the rollback target, since
  // a system choice renders differently under each OS preference).
  // Hosts without matchMedia (some test shells) skip the watcher.
  React.useEffect(() => {
    applyChoice(choice.value, choice.custom);
    if (choice.value !== "system") return undefined;
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") return undefined;
    return watchSystemTheme(() => {
      const next = applyChoice("system", undefined);
      committed.current = { choice: { value: "system", custom: undefined }, scheme: next };
      setApplied({ scheme: next, custom: undefined });
    });
  }, [choice]);

  // One-time server sync per user: the profile wins once, then local
  // choices drive until the next sign-in. Later profile refetches for
  // the same user never yank the applied theme back.
  const serverValue = profileQuery.data?.theme;
  const serverUser = profileQuery.data?.user;
  React.useEffect(() => {
    if (serverUser === undefined || serverValue === undefined) return;
    if (syncedUser.current === serverUser) return;
    syncedUser.current = serverUser;
    const bag = readThemeBag(serverValue);
    const custom = bag.value === "custom" ? bag.custom : undefined;
    const scheme = applyChoice(bag.value, custom);
    setChoice({ value: bag.value, custom });
    setApplied({ scheme, custom });
    committed.current = { choice: { value: bag.value, custom }, scheme };
    if (custom !== undefined) writeStoredPalette(custom);
    writeStoredTheme(bag.value);
  }, [serverUser, serverValue]);

  const context = React.useMemo<ThemeContextValue>(
    () => ({ value: choice.value, resolved, custom: choice.custom, isSaving: save.isPending, setTheme }),
    [choice, resolved, save.isPending, setTheme]
  );
  return <ThemeContext.Provider value={context}>{children}</ThemeContext.Provider>;
}
