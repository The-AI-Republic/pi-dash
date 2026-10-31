// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Interface language provider (shell spec, SHELL-096). Holds the
// active tag in context: switching stamps the document, persists
// locally and on the profile, and re-renders every subscriber. The
// server value syncs once per user; unknown tags (from fuller builds'
// profiles) fall back to English rather than breaking the shell.
// The error notifier arrives by prop so this core module never
// imports the shared toaster.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { updateProfile } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../api/client.js";
import { sessionKeys, useProfile } from "../session/index.js";
import {
  applyDocumentLanguage,
  isSupportedLocale,
  readStoredLanguage,
  resolveInitialLanguage,
  supportedLocales,
  writeStoredLanguage,
  type LocaleOption,
} from "./locales.js";

export interface LocaleContextValue {
  /** The active BCP 47 tag. */
  tag: string;
  /** Locales this build offers, in picker order. */
  supported: LocaleOption[];
  /** True while the profile write is in flight. */
  isSaving: boolean;
  /**
   * Switch language: stamps the document and stores locally at once,
   * then persists to the profile. Unknown tags and failed writes toast
   * and keep the current tag. Resolves true once the write lands and
   * false when it is rejected, so a caller can confirm only on success.
   */
  setLanguage: (tag: string) => Promise<boolean>;
}

const LocaleContext = React.createContext<LocaleContextValue | null>(null);

/** Read the interface language; must render under LanguageProvider. */
export function useLocale(): LocaleContextValue {
  const context = React.useContext(LocaleContext);
  if (context === null) throw new Error("useLocale must render under LanguageProvider");
  return context;
}

export interface LanguageProviderProps {
  children: React.ReactNode;
  /** Error surface for rejected tags and failed writes (the shared toaster). */
  notifyError: (title: string, description?: string) => void;
}

export function LanguageProvider({ children, notifyError }: LanguageProviderProps): React.ReactElement {
  const queryClient = useQueryClient();
  const profileQuery = useProfile();
  const [tag, setTag] = React.useState<string>(() => readStoredLanguage());
  const syncedUser = React.useRef<string | null>(null);

  const save = useMutation({
    mutationFn: async (next: string) => updateProfile(getAppClient(), { language: next }),
    onSuccess: (profile) => {
      queryClient.setQueryData(sessionKeys.profile, profile);
    },
    onError: (error: Error) => {
      notifyError("Couldn't update the language", error.message);
    },
  });

  const setLanguage = React.useCallback(
    (next: string): Promise<boolean> => {
      if (!isSupportedLocale(next)) {
        notifyError("Couldn't update the language", `“${next}” is not available in this build.`);
        return Promise.resolve(false);
      }
      setTag(next);
      applyDocumentLanguage(next);
      writeStoredLanguage(next);
      return save.mutateAsync(next).then(
        () => true,
        () => false
      );
    },
    [save, notifyError]
  );

  // Boot stamp plus one-time server sync per user. Later profile
  // refetches for the same user never yank the language back.
  const serverLanguage = profileQuery.data?.language;
  const serverUser = profileQuery.data?.user;
  React.useEffect(() => {
    if (typeof document !== "undefined") applyDocumentLanguage(tag);
    if (serverUser === undefined || serverLanguage === undefined) return;
    if (syncedUser.current === serverUser) return;
    syncedUser.current = serverUser;
    const next = resolveInitialLanguage(readStoredLanguage(), serverLanguage);
    setTag(next);
    applyDocumentLanguage(next);
    writeStoredLanguage(next);
  }, [serverUser, serverLanguage, tag]);

  const context = React.useMemo<LocaleContextValue>(
    () => ({ tag, supported: supportedLocales(), isSaving: save.isPending, setLanguage }),
    [tag, save.isPending, setLanguage]
  );
  return <LocaleContext.Provider value={context}>{children}</LocaleContext.Provider>;
}
