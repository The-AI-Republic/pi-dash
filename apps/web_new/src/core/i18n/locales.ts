// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Interface language model (shell spec, SHELL-096). The open build
// ships English only; fuller builds register more locales through the
// same table (never by replacing files). The choice persists twice:
// the profile language on the server and a local key (shared with the
// old app's key so the two agree during the migration). Switching
// stamps the document language and notifies every subscriber, so the
// whole shell re-renders.

export interface LocaleOption {
  /** BCP 47 tag stored on the profile and locally. */
  tag: string;
  /** Human name shown in pickers. */
  label: string;
}

/** localStorage key for the chosen tag. */
export const LANGUAGE_STORAGE_KEY = "userLanguage";

const FALLBACK_TAG = "en";

const registered: LocaleOption[] = [{ tag: "en", label: "English" }];

/** Locales the current build offers, in picker order. */
export function supportedLocales(): LocaleOption[] {
  return [...registered];
}

/**
 * Add a locale to this build's offer (fuller builds call this at
 * bootstrap; the open build never does). Re-registering a tag
 * replaces its label.
 */
export function registerLocale(option: LocaleOption): void {
  const at = registered.findIndex((entry) => entry.tag === option.tag);
  if (at === -1) registered.push({ ...option });
  else registered[at] = { ...option };
}

/** Drop test-only registrations (tests only; the app never unregisters). */
export function resetLocalesForTests(): void {
  registered.splice(1);
  registered[0] = { tag: "en", label: "English" };
}

/** True when the build offers the tag. */
export function isSupportedLocale(tag: string): boolean {
  return registered.some((entry) => entry.tag === tag);
}

function storage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

/** The locally stored tag, or English when missing, unknown or unreadable. */
export function readStoredLanguage(): string {
  try {
    const raw = storage()?.getItem(LANGUAGE_STORAGE_KEY);
    return typeof raw === "string" && isSupportedLocale(raw) ? raw : FALLBACK_TAG;
  } catch {
    return FALLBACK_TAG;
  }
}

/** Persist the tag locally; private modes silently keep the default. */
export function writeStoredLanguage(tag: string): void {
  try {
    storage()?.setItem(LANGUAGE_STORAGE_KEY, tag);
  } catch {
    // Dismissed: the server copy still holds the choice.
  }
}

/** Stamp the document language for assistive tech and the gate driver. */
export function applyDocumentLanguage(tag: string, target?: HTMLElement): void {
  (target ?? document.documentElement).setAttribute("lang", tag);
}

/**
 * Resolve the starting tag: the stored choice wins, then the server
 * profile, then English. Unknown tags never escape this function.
 */
export function resolveInitialLanguage(stored: string, server: string | undefined): string {
  if (isSupportedLocale(stored)) return stored;
  if (server !== undefined && isSupportedLocale(server)) return server;
  return FALLBACK_TAG;
}
