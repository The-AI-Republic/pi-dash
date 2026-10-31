// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

export { LanguageProvider, useLocale } from "./LanguageProvider.js";
export type { LanguageProviderProps, LocaleContextValue } from "./LanguageProvider.js";
export {
  applyDocumentLanguage,
  isSupportedLocale,
  LANGUAGE_STORAGE_KEY,
  readStoredLanguage,
  registerLocale,
  resetLocalesForTests,
  resolveInitialLanguage,
  supportedLocales,
  writeStoredLanguage,
} from "./locales.js";
export type { LocaleOption } from "./locales.js";
