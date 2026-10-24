// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Reusable peek chrome (parity/specs/issues.md §14, ISS-166/168). The
// shared panel shell plus the display-mode toggle: side, modal, and
// full presentations with guarded Escape and outside-click close.
// Issues mounts its full detail body here; later areas (modules, space)
// mount their own bodies through the same chrome. Embed mode disables
// both close paths.

import * as React from "react";

import { PeekPanel } from "../../../shared/shell/PeekPanel.js";
import { PeekModeToggle, type PeekMode } from "./PeekModeToggle.js";

export interface PeekChromeProps {
  mode: PeekMode;
  onModeChange: (mode: PeekMode) => void;
  /** Current title; the field commits through onCommitTitle. */
  title: string;
  /** Commits a renamed title; without it edits stay local to the field. */
  onCommitTitle?: ((next: string) => void) | undefined;
  /** Render the title as a heading for read-only surfaces. */
  titleAsHeading?: boolean | undefined;
  /** Context line above the title (e.g. the work-item key). */
  eyebrow?: React.ReactNode;
  /** Controls between the mode toggle and the close button. */
  actions?: React.ReactNode;
  onClose: () => void;
  /** Caller-known Escape suppression (a nested peek above this one). */
  suppressEscape?: boolean | undefined;
  /** Embedded hosts disable Escape and outside-click close. */
  embedded?: boolean | undefined;
  /** Test handle for the panel; nested panels take their own. */
  testId?: string | undefined;
  /** A nested panel paints above its parent. */
  stacked?: boolean | undefined;
  children?: React.ReactNode;
}

export function PeekChrome({
  mode,
  onModeChange,
  title,
  onCommitTitle,
  titleAsHeading = false,
  eyebrow,
  actions,
  onClose,
  suppressEscape = false,
  embedded = false,
  testId,
  stacked = false,
  children,
}: PeekChromeProps): React.ReactElement {
  return (
    <PeekPanel
      layout={mode}
      stacked={stacked}
      eyebrow={eyebrow}
      title={title}
      titleLabel="Work item title"
      titleAsHeading={titleAsHeading}
      onClose={onClose}
      suppressEscape={suppressEscape || embedded}
      {...(embedded ? {} : { onOutsideClick: onClose })}
      {...(testId === undefined ? {} : { testId })}
      {...(onCommitTitle === undefined ? {} : { onCommitTitle })}
      actions={
        <>
          <PeekModeToggle value={mode} onChange={onModeChange} />
          {actions}
        </>
      }
    >
      {children}
    </PeekPanel>
  );
}
