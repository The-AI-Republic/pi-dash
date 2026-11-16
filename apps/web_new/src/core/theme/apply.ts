// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Theme document application (shell spec, SHELL-095). One writer owns
// the root theme mark: data-theme always carries the resolved scheme
// (system writes light or dark, never a placeholder), color-scheme
// follows for form controls, and custom palettes ride as variables
// the custom block derives tokens from. Clearing a custom palette
// removes its variables so the cascade falls back cleanly.

import type { CustomPalette, ResolvedScheme } from "./theme.js";

const THEME_ATTRIBUTE = "data-theme";
const CUSTOM_PRIMARY_PROPERTY = "--theme-custom-primary";
const CUSTOM_BACKGROUND_PROPERTY = "--theme-custom-background";
const CUSTOM_MODE_PROPERTY = "--theme-custom-mode";

/** Apply a resolved scheme plus its custom palette, if any. */
export function applyResolvedTheme(
  scheme: ResolvedScheme,
  custom: CustomPalette | undefined,
  target: HTMLElement
): void {
  target.setAttribute(THEME_ATTRIBUTE, scheme);
  const dark = scheme === "dark" || scheme === "dark-contrast" || (scheme === "custom" && custom?.darkPalette === true);
  target.style.setProperty("color-scheme", dark ? "dark" : "light");
  if (scheme === "custom" && custom !== undefined) {
    target.style.setProperty(CUSTOM_PRIMARY_PROPERTY, custom.primary);
    target.style.setProperty(CUSTOM_BACKGROUND_PROPERTY, custom.background);
    target.style.setProperty(CUSTOM_MODE_PROPERTY, custom.darkPalette ? "dark" : "light");
  } else {
    clearCustomPalette(target);
  }
}

/** Drop the custom variables (leaves the scheme mark alone). */
export function clearCustomPalette(target: HTMLElement): void {
  target.style.removeProperty(CUSTOM_PRIMARY_PROPERTY);
  target.style.removeProperty(CUSTOM_BACKGROUND_PROPERTY);
  target.style.removeProperty(CUSTOM_MODE_PROPERTY);
}

/** Whether the OS currently prefers a dark scheme. */
export function prefersDarkScheme(): boolean {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return false;
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}
