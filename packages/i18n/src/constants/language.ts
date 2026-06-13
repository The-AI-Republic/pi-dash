/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { TLanguage, ILanguageOption } from "../types";
import { locales } from "../locales";

export const FALLBACK_LANGUAGE: TLanguage = "en";

/**
 * Display names (endonyms) for every language the codebase knows about. These
 * are metadata, not translated content, so the full table is kept here even
 * though the open-source build ships English only — it lets locales layered in
 * on top (see `locales/index.ts`) render with the right label automatically.
 */
const LANGUAGE_LABELS: Record<TLanguage, string> = {
  en: "English",
  fr: "Français",
  es: "Español",
  ja: "日本語",
  "zh-CN": "简体中文",
  "zh-TW": "繁體中文",
  ru: "Русский",
  it: "Italian",
  cs: "Čeština",
  sk: "Slovenčina",
  de: "Deutsch",
  ua: "Українська",
  pl: "Polski",
  ko: "한국어",
  "pt-BR": "Português Brasil",
  id: "Indonesian",
  ro: "Română",
  "vi-VN": "Tiếng việt",
  "tr-TR": "Türkçe",
};

/**
 * The languages actually available at runtime — derived from the `locales` map.
 * The OSS build exposes English only; the cloud overlay swaps in a fuller
 * `locales` map at build time and these options expand to match, with no change
 * to consumers (the language picker reads this list).
 */
export const SUPPORTED_LANGUAGES: ILanguageOption[] = (Object.keys(locales) as TLanguage[]).map((value) => ({
  label: LANGUAGE_LABELS[value] ?? value,
  value,
}));

/**
 * Enum for translation file names
 * These are the JSON files that contain translations each category
 */
export enum ETranslationFiles {
  TRANSLATIONS = "translations",
  ACCESSIBILITY = "accessibility",
  EDITOR = "editor",
  EMPTY_STATE = "empty-state",
}

export const LANGUAGE_STORAGE_KEY = "userLanguage";
