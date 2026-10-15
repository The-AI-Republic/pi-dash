// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Appearance theme model (shell spec, SHELL-095). Six stored values: the
// system follower, light and dark, a high-contrast pair, and custom
// palettes. The value persists twice: the profile theme bag on the
// server and a local key (shared with the old app's key so the two
// agree during the route-by-route migration). The document root always
// carries the resolved scheme, so readers never guess.

/** Stored theme values; the profile bag and the local key agree on these. */
export const THEME_VALUES = ["system", "light", "dark", "light-contrast", "dark-contrast", "custom"] as const;
export type ThemeValue = (typeof THEME_VALUES)[number];

/** Palette options in display order with their labels. */
export const THEME_OPTIONS: ReadonlyArray<{ value: ThemeValue; label: string }> = [
  { value: "system", label: "System preference" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "light-contrast", label: "Light high contrast" },
  { value: "dark-contrast", label: "Dark high contrast" },
  { value: "custom", label: "Custom theme" },
];

/** localStorage key for the chosen value. */
export const THEME_STORAGE_KEY = "theme";

/** Resolved document schemes (what data-theme carries). */
export type ResolvedScheme = "light" | "dark" | "light-contrast" | "dark-contrast" | "custom";

/** Whether a stored value names a dark base (for icon and art swaps). */
export function isDarkValue(value: ThemeValue, prefersDark: boolean): boolean {
  if (value === "dark" || value === "dark-contrast") return true;
  if (value === "light" || value === "light-contrast") return false;
  return prefersDark;
}

/**
 * Resolve a stored value to its document scheme. System follows the OS;
 * custom carries its own scheme with palette variables beside it.
 */
export function resolveScheme(value: ThemeValue, prefersDark: boolean): ResolvedScheme {
  switch (value) {
    case "light":
      return "light";
    case "dark":
      return "dark";
    case "light-contrast":
      return "light-contrast";
    case "dark-contrast":
      return "dark-contrast";
    case "custom":
      return "custom";
    case "system":
      return prefersDark ? "dark" : "light";
  }
}

/** Parse a stored value; anything unknown falls back to following the OS. */
export function parseThemeValue(raw: unknown): ThemeValue {
  return (THEME_VALUES as ReadonlyArray<unknown>).includes(raw) ? (raw as ThemeValue) : "system";
}

function storage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

/** The locally stored choice, or system when missing or unreadable. */
export function readStoredTheme(): ThemeValue {
  try {
    return parseThemeValue(storage()?.getItem(THEME_STORAGE_KEY));
  } catch {
    return "system";
  }
}

/** Persist the choice locally; private modes silently keep the default. */
export function writeStoredTheme(value: ThemeValue): void {
  try {
    storage()?.setItem(THEME_STORAGE_KEY, value);
  } catch {
    // Dismissed: the server copy still holds the choice.
  }
}

/** Custom palette carried beside the value in the profile theme bag. */
export interface CustomPalette {
  primary: string;
  background: string;
  darkPalette: boolean;
}

/** The profile theme bag narrowed to what the shell reads and writes. */
export interface ThemeBag {
  value: ThemeValue;
  custom?: CustomPalette | undefined;
}

/** Read the shell's slice of the profile theme bag (never throws). */
export function readThemeBag(bag: Record<string, unknown> | null | undefined): ThemeBag {
  if (bag === null || bag === undefined) return { value: "system" };
  const value = parseThemeValue(bag["theme"]);
  if (value !== "custom") return { value };
  const primary = bag["primary"];
  const background = bag["background"];
  if (typeof primary !== "string" || typeof background !== "string") return { value };
  return { value, custom: { primary, background, darkPalette: bag["darkPalette"] === true } };
}

/**
 * Build the profile theme bag the server stores. Existing keys carry
 * over, so switching between stock themes never wipes a saved custom
 * palette (and switching to custom keeps the stored value's company).
 */
export function buildThemeBag(
  value: ThemeValue,
  custom: CustomPalette | undefined,
  existing: Record<string, unknown> = {}
): Record<string, unknown> {
  if (value !== "custom" || custom === undefined) return { ...existing, theme: value };
  return {
    ...existing,
    theme: value,
    primary: custom.primary,
    background: custom.background,
    darkPalette: custom.darkPalette,
  };
}
