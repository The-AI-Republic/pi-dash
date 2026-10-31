// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runtime theme and density helpers for @pidash/kit. The visual values live
// in tokens.css; these helpers only switch the data attributes that select
// them. Theme follows the OS by default; density defaults to compact.

export type ThemeMode = "light" | "dark" | "system";
export type Density = "compact" | "comfortable";

const THEME_ATTRIBUTE = "data-theme";
const DENSITY_ATTRIBUTE = "data-density";

function rootElement(): HTMLElement {
  return document.documentElement;
}

/** Resolved theme for a mode: system defers to the OS preference. */
export function resolveTheme(mode: ThemeMode): "light" | "dark" {
  if (mode !== "system") return mode;
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
    return "light";
  }
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

/**
 * Apply a theme mode. System removes the pin so tokens.css follows the OS
 * live (including desktop OS changes); light/dark pin the value.
 */
export function applyTheme(mode: ThemeMode, target: HTMLElement = rootElement()): void {
  if (mode === "system") {
    target.removeAttribute(THEME_ATTRIBUTE);
  } else {
    target.setAttribute(THEME_ATTRIBUTE, mode);
  }
}

/** Apply a density stop; compact is the default (desktop-first). */
export function applyDensity(density: Density, target: HTMLElement = rootElement()): void {
  target.setAttribute(DENSITY_ATTRIBUTE, density);
}

/**
 * Watch the OS theme and call back on changes. The desktop build uses this
 * to track OS changes live. Returns an unsubscribe function.
 */
export function watchSystemTheme(listener: (theme: "light" | "dark") => void): () => void {
  const query = window.matchMedia("(prefers-color-scheme: dark)");
  const handler = (event: MediaQueryListEvent): void => {
    listener(event.matches ? "dark" : "light");
  };
  query.addEventListener("change", handler);
  return () => {
    query.removeEventListener("change", handler);
  };
}
