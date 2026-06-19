/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// The open-source build ships English only. Additional locales are layered in
// by adding a `<lang>/` directory here and a matching entry in the `locales`
// map below — see README.md. `SUPPORTED_LANGUAGES` derives from this map, so a
// new entry automatically appears in the language picker.

// Export all locale files to make them accessible from the package root
export { default as enCore } from "./en/core";
export { default as enTranslations } from "./en/translations";
export { default as enAccessibility } from "./en/accessibility";
export { default as enEditor } from "./en/editor";
export { default as enEmptyState } from "./en/empty-state";

// Export locale data for all supported languages
export const locales = {
  en: {
    core: () => import("./en/core"),
    translations: () => import("./en/translations"),
    accessibility: () => import("./en/accessibility"),
    editor: () => import("./en/editor"),
    "empty-state": () => import("./en/empty-state"),
  },
};
