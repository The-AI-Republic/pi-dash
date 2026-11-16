// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Subtree locale (parity/specs/space.md SPC-065). The space subtree
// cannot mount the workspace language provider — a visitor has no
// profile to sync or persist to, and its anonymous reads must never
// trip the session gate — so it resolves the tag from the same stored
// key through the session-free locale helpers: an unsupported stored
// tag heals to English on boot, the document language is stamped, and
// supported tags (cloud packs) pass through untouched. Identifiers,
// server names and date shapes are data and never translated.
import * as React from "react";

import {
  applyDocumentLanguage,
  isSupportedLocale,
  LANGUAGE_STORAGE_KEY,
  readStoredLanguage,
  writeStoredLanguage,
} from "../../../core/i18n/locales.js";

/** True when a stored tag exists that this build does not offer. */
export function storedSpaceLocaleNeedsHeal(): boolean {
  try {
    const raw = globalThis.localStorage?.getItem(LANGUAGE_STORAGE_KEY);
    return typeof raw === "string" && !isSupportedLocale(raw);
  } catch {
    return false;
  }
}

/**
 * Resolve the subtree tag for this load: heal an unsupported stored
 * value to English, stamp the document language, and return the tag.
 */
export function healSpaceLocale(): string {
  const tag = readStoredLanguage();
  applyDocumentLanguage(tag);
  if (storedSpaceLocaleNeedsHeal()) writeStoredLanguage(tag);
  return tag;
}

/**
 * Subtree locale tag for later slices (dates, counts). Heals once on
 * mount; the tag is static per load — spaces offers no switcher.
 */
export function useSpaceLocaleTag(): string {
  const [tag, setTag] = React.useState(readStoredLanguage);
  React.useEffect(() => {
    setTag(healSpaceLocale());
  }, []);
  return tag;
}
