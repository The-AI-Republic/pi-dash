// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shortcut presentation and firing guards (shell spec, SHELL-086/087/094).
// Sequences render as uppercase key chips. Global chords and sequences
// never fire while the user is typing (the palette chord stays exempt).
// Chord glyphs live with the shortcuts dialog, its only consumer, so the
// glyph table rides the deferred dialog chunk (Quality gates budget).

/** Display chips for a two-key sequence ("op" → ["O", "P"]). */
export function formatSequenceKeys(sequence: string): string[] {
  return sequence.split("").map((key) => key.toUpperCase());
}

interface TypingTarget {
  tagName?: string;
  classList?: { contains(name: string): boolean };
  getAttribute?: (name: string) => string | null;
  isContentEditable?: boolean;
}

/**
 * Whether keyboard focus sits in a typing surface: inputs, text areas,
 * content-editable regions and the ProseMirror editor. Global chords and
 * sequences stay silent there (SHELL-086/087); only the palette chord
 * fires while typing (SHELL-080).
 */
export function isTypingTarget(target: EventTarget | null): boolean {
  if (target === null || typeof target !== "object") return false;
  const element = target as TypingTarget;
  const tag = element.tagName?.toUpperCase();
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  if (element.classList?.contains("ProseMirror") === true) return true;
  if (element.isContentEditable === true) return true;
  return element.getAttribute?.("contenteditable") === "true";
}
