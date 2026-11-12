// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import * as React from "react";
import { cn } from "../lib/cn";

/** True on macOS, where the mod key renders as ⌘. */
export function isMacPlatform(): boolean {
  if (typeof navigator === "undefined" || typeof navigator.platform !== "string") return false;
  return navigator.platform.toUpperCase().includes("MAC");
}

/**
 * Display glyph for one raw key name ("mod" → "⌘" on macOS, "Ctrl"
 * elsewhere). Unknown names render uppercased, so a malformed key still
 * shows something instead of vanishing.
 */
export function formatKbdKey(key: string, mac: boolean): string {
  const raw = key.trim();
  switch (raw.toLowerCase()) {
    case "mod":
    case "meta":
    case "cmd":
      return mac ? "⌘" : "Ctrl";
    case "ctrl":
      return mac ? "⌃" : "Ctrl";
    case "alt":
    case "option":
      return mac ? "⌥" : "Alt";
    case "shift":
      return mac ? "⇧" : "Shift";
    case "":
      return "Space";
    default:
      return raw.toUpperCase();
  }
}

/**
 * Spoken name for one raw key name ("mod" → "Command" on macOS, "Ctrl"
 * elsewhere), so the shortcut's accessible name reads the real keys
 * instead of glyphs or raw names.
 */
export function kbdKeyLabel(key: string, mac: boolean): string {
  const raw = key.trim();
  switch (raw.toLowerCase()) {
    case "mod":
    case "meta":
    case "cmd":
      return mac ? "Command" : "Ctrl";
    case "ctrl":
      return mac ? "Control" : "Ctrl";
    case "alt":
    case "option":
      return mac ? "Option" : "Alt";
    case "shift":
      return "Shift";
    case "":
      return "Space";
    default:
      return raw.toUpperCase();
  }
}

export interface KbdProps {
  /** Raw key names in press order, e.g. ["mod", "K"]. */
  keys: string[];
  className?: string;
}

export function Kbd({ keys, className }: KbdProps): React.ReactElement {
  const mac = isMacPlatform();
  return (
    <span
      aria-label={keys.map((key) => kbdKeyLabel(key, mac)).join("+")}
      className={cn("inline-flex items-center gap-(--space-1)", className)}
    >
      {keys.map((key, index) => (
        <React.Fragment key={`${index}-${key}`}>
          {index > 0 ? (
            <span aria-hidden="true" className="text-caption text-(--text-muted)">
              +
            </span>
          ) : null}
          <kbd className="text-caption rounded-(--radius-control) border border-(--border) bg-(--subtle) px-(--space-2) py-px font-[family-name:var(--font-mono)] text-(--text)">
            {formatKbdKey(key, mac)}
          </kbd>
        </React.Fragment>
      ))}
    </span>
  );
}
