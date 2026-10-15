// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Appearance theme provider (shell spec, SHELL-095). Owns the root
// theme mark for the session: the stored choice applies at boot, the
// server profile syncs once per user, the OS is tracked while the
// system choice holds, and every change re-skins immediately (no
// reload). Failures toast and keep the local choice; the next boot
// re-syncs from the server. The error notifier arrives by prop so this
// core module never imports the shared toaster.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { updateProfile } from "@pidash/api-client";
import { watchSystemTheme } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../api/client.js";
import { sessionKeys, useProfile } from "../session/index.js";
import { applyResolvedTheme, prefersDarkScheme } from "./apply.js";
import {
  buildThemeBag,
  readStoredTheme,
  readThemeBag,
  resolveScheme,
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
   * to the profile. A failed write toasts and keeps the local choice.
   */
  setTheme: (value: ThemeValue, custom?: CustomPalette | undefined) => void;
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

function applyChoice(value: ThemeValue, custom: CustomPalette | undefined): ResolvedScheme {
  const scheme = resolveScheme(value, prefersDarkScheme());
  applyResolvedTheme(scheme, value === "custom" ? custom : undefined, rootElement());
  return scheme;
}

export interface ThemeProviderProps {
  children: React.ReactNode;
  /** Error surface for failed profile writes (the shared toaster). */
  notifyError: (title: string, description?: string) => void;
}

export function ThemeProvider({ children, notifyError }: ThemeProviderProps): React.ReactElement {
  const queryClient = useQueryClient();
  const profileQuery = useProfile();
  const [choice, setChoice] = React.useState<{ value: ThemeValue; custom: CustomPalette | undefined }>(() => ({
    value: readStoredTheme(),
    custom: undefined,
  }));
  const [resolved, setResolved] = React.useState<ResolvedScheme>(() =>
    resolveScheme(choice.value, prefersDarkScheme())
  );
  const syncedUser = React.useRef<string | null>(null);

  const save = useMutation({
    mutationFn: async (bag: Record<string, unknown>) => updateProfile(getAppClient(), { theme: bag }),
    onSuccess: (profile) => {
      queryClient.setQueryData(sessionKeys.profile, profile);
    },
    onError: (error: Error) => {
      notifyError("Couldn't update the theme", error.message);
    },
  });

  const cachedBag = profileQuery.data?.theme;
  const setTheme = React.useCallback(
    (value: ThemeValue, custom: CustomPalette | undefined = undefined) => {
      const next = value === "custom" ? custom : undefined;
      setChoice({ value, custom: next });
      setResolved(applyChoice(value, next));
      writeStoredTheme(value);
      save.mutate(buildThemeBag(value, next, cachedBag ?? {}));
    },
    [save, cachedBag]
  );

  // Boot mark plus OS tracking while the system choice holds. The OS
  // watcher re-applies on flips; leaving system stops the tracking.
  // Hosts without matchMedia (some test shells) skip the watcher.
  React.useEffect(() => {
    setResolved(applyChoice(choice.value, choice.custom));
    if (choice.value !== "system") return undefined;
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") return undefined;
    return watchSystemTheme(() => {
      setResolved(applyChoice("system", undefined));
    });
  }, [choice.value, choice.custom]);

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
    setChoice({ value: bag.value, custom: bag.custom });
    setResolved(applyChoice(bag.value, bag.custom));
    writeStoredTheme(bag.value);
  }, [serverUser, serverValue]);

  const context = React.useMemo<ThemeContextValue>(
    () => ({ value: choice.value, resolved, custom: choice.custom, isSaving: save.isPending, setTheme }),
    [choice, resolved, save.isPending, setTheme]
  );
  return <ThemeContext.Provider value={context}>{children}</ThemeContext.Provider>;
}
