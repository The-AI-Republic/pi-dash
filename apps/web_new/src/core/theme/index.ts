// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

export { applyResolvedTheme, clearCustomPalette, prefersDarkScheme } from "./apply.js";
export {
  CustomThemeDesigner,
  normalizeHexColor,
  parseThemeConfigFile,
  serializeThemeConfig,
} from "./CustomThemeDesigner.js";
export type { CustomThemeDesignerProps } from "./CustomThemeDesigner.js";
export {
  buildThemeBag,
  clearStoredPalette,
  isDarkValue,
  parseThemeValue,
  readStoredPalette,
  readStoredTheme,
  readThemeBag,
  resolveScheme,
  THEME_PALETTE_STORAGE_KEY,
  THEME_OPTIONS,
  THEME_STORAGE_KEY,
  THEME_VALUES,
  writeStoredPalette,
  writeStoredTheme,
} from "./theme.js";
export type { CustomPalette, ResolvedScheme, ThemeBag, ThemeValue } from "./theme.js";
export { readDocumentScheme, ThemeProvider, useResolvedScheme, useTheme } from "./ThemeProvider.js";
export type { ThemeContextValue, ThemeProviderProps } from "./ThemeProvider.js";
