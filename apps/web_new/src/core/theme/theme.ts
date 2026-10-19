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

/** Custom palette carried beside the value in the profile theme bag. */
export interface CustomPalette {
  primary: string;
  background: string;
  darkPalette: boolean;
}

/** Persist the choice locally; private modes silently keep the default. */
export function writeStoredTheme(value: ThemeValue): void {
  try {
    storage()?.setItem(THEME_STORAGE_KEY, value);
  } catch {
    // Dismissed: the server copy still holds the choice.
  }
}

/**
 * The saved custom palette carried beside the local choice. Without it a
 * reload that lands on the custom value would paint the fallback colors
 * until the profile read arrives, so the picker stores it with the value.
 */
export const THEME_PALETTE_STORAGE_KEY = "theme-palette";

/** The locally stored custom palette, or undefined when missing or unreadable. */
export function readStoredPalette(): CustomPalette | undefined {
  try {
    const raw = storage()?.getItem(THEME_PALETTE_STORAGE_KEY);
    if (raw === null || raw === undefined) return undefined;
    const parsed = JSON.parse(raw) as unknown;
    if (typeof parsed !== "object" || parsed === null) return undefined;
    const record = parsed as Record<string, unknown>;
    const primary = record["primary"];
    const background = record["background"];
    if (typeof primary !== "string" || typeof background !== "string") return undefined;
    return { primary, background, darkPalette: record["darkPalette"] === true };
  } catch {
    return undefined;
  }
}

/** Persist the custom palette locally; private modes keep the server copy. */
export function writeStoredPalette(palette: CustomPalette): void {
  try {
    storage()?.setItem(THEME_PALETTE_STORAGE_KEY, JSON.stringify(palette));
  } catch {
    // Dismissed: the profile read still carries the saved colors.
  }
}

/** The profile theme bag narrowed to what the shell reads and writes. */
export interface ThemeBag {
  value: ThemeValue;
  custom?: CustomPalette | undefined;
}

/**
 * Read the shell's slice of the profile theme bag (never throws). The
 * saved custom colors ride alongside any chosen value: switching to a
 * stock theme keeps them, so picking custom again can re-apply them.
 */
export function readThemeBag(bag: Record<string, unknown> | null | undefined): ThemeBag {
  if (bag === null || bag === undefined) return { value: "system" };
  const value = parseThemeValue(bag["theme"]);
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
