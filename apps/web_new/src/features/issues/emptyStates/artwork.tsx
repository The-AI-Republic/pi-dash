// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Inline art for the issues empty states (parity/specs/issues.md §8). Each
// drawing uses the theme tokens so the light, dark and high-contrast
// schemes stay legible, carries no text, and sizes to the detailed tier's
// art slot. Shapes are deliberately spare: a visible outline, one accent
// element and the subject the card talks about.

import * as React from "react";

function Scene({ children, name }: { children: React.ReactNode; name: string }): React.ReactElement {
  return (
    <svg viewBox="0 0 160 120" width="160" height="120" aria-hidden="true" data-artwork={name} focusable="false">
      {children}
    </svg>
  );
}

/** An empty sheet with a plus: nothing has been filed yet. */
export function FirstItemArt(): React.ReactElement {
  return (
    <Scene name="First work item">
      <rect x="26" y="16" width="108" height="88" rx="8" fill="var(--surface)" stroke="var(--border)" strokeWidth="2" />
      <rect x="40" y="34" width="58" height="6" rx="3" fill="var(--subtle)" />
      <rect x="40" y="48" width="80" height="6" rx="3" fill="var(--subtle)" />
      <rect x="40" y="62" width="68" height="6" rx="3" fill="var(--subtle)" />
      <circle cx="118" cy="86" r="16" fill="var(--accent)" />
      <path d="M118 78v16M110 86h16" stroke="var(--on-accent)" strokeWidth="3" strokeLinecap="round" />
    </Scene>
  );
}

/** A magnifier over an empty sheet: the filters matched nothing. */
export function NoMatchArt(): React.ReactElement {
  return (
    <Scene name="No matching results">
      <rect x="22" y="16" width="92" height="80" rx="8" fill="var(--surface)" stroke="var(--border)" strokeWidth="2" />
      <rect x="36" y="34" width="48" height="6" rx="3" fill="var(--subtle)" />
      <rect x="36" y="48" width="64" height="6" rx="3" fill="var(--subtle)" />
      <circle cx="104" cy="76" r="20" fill="var(--surface)" stroke="var(--accent)" strokeWidth="4" />
      <path d="M118 90l14 14" stroke="var(--accent)" strokeWidth="5" strokeLinecap="round" />
    </Scene>
  );
}

/** A box holding a finished sheet: the archive after its first filing. */
export function ArchiveArt(): React.ReactElement {
  return (
    <Scene name="Archived work items">
      <rect x="28" y="40" width="104" height="58" rx="5" fill="var(--surface)" stroke="var(--border)" strokeWidth="2" />
      <rect
        x="22"
        y="24"
        width="116"
        height="14"
        rx="4"
        fill="var(--accent-subtle)"
        stroke="var(--accent)"
        strokeWidth="2"
      />
      <path
        d="M66 66l10 10 20-22"
        fill="none"
        stroke="var(--accent)"
        strokeWidth="4"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </Scene>
  );
}

/** Layered frames with a star: a saved view waiting for its first rows. */
export function ViewArt(): React.ReactElement {
  return (
    <Scene name="Saved view">
      <rect x="42" y="26" width="84" height="60" rx="6" fill="var(--subtle)" stroke="var(--border)" strokeWidth="2" />
      <rect x="34" y="18" width="84" height="60" rx="6" fill="var(--surface)" stroke="var(--border)" strokeWidth="2" />
      <rect x="46" y="34" width="48" height="6" rx="3" fill="var(--subtle)" />
      <rect x="46" y="46" width="36" height="6" rx="3" fill="var(--subtle)" />
      <path d="M120 78l4 9 9 4-9 4-4 9-4-9-9-4 9-4Z" fill="var(--accent)" />
    </Scene>
  );
}

/** Two project cards laid over each other: a workspace before its first project. */
export function ProjectArt(): React.ReactElement {
  return (
    <Scene name="No projects">
      <rect x="22" y="36" width="62" height="52" rx="6" fill="var(--subtle)" stroke="var(--border)" strokeWidth="2" />
      <rect x="44" y="24" width="62" height="52" rx="6" fill="var(--surface)" stroke="var(--border)" strokeWidth="2" />
      <rect
        x="68"
        y="12"
        width="62"
        height="52"
        rx="6"
        fill="var(--accent-subtle)"
        stroke="var(--accent)"
        strokeWidth="2"
      />
      <rect x="80" y="26" width="30" height="5" rx="2.5" fill="var(--accent)" opacity="0.7" />
      <rect x="30" y="98" width="100" height="6" rx="3" fill="var(--subtle)" />
    </Scene>
  );
}

/** A clipboard with a blank page: a profile tab with nothing to list. */
export function ProfileArt(): React.ReactElement {
  return (
    <Scene name="Profile work items">
      <rect x="44" y="18" width="72" height="88" rx="8" fill="var(--surface)" stroke="var(--border)" strokeWidth="2" />
      <rect x="62" y="10" width="36" height="14" rx="4" fill="var(--accent)" />
      <rect x="56" y="44" width="48" height="6" rx="3" fill="var(--subtle)" />
      <rect x="56" y="58" width="36" height="6" rx="3" fill="var(--subtle)" />
      <rect x="56" y="72" width="42" height="6" rx="3" fill="var(--subtle)" />
    </Scene>
  );
}
