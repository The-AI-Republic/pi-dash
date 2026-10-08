// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import * as React from "react";
import { cn } from "../lib/cn";

export interface KbdProps {
  /** Key names in press order, e.g. ["mod", "K"]. */
  keys: string[];
  className?: string;
}

export function Kbd({ keys, className }: KbdProps): React.ReactElement {
  return (
    <span aria-label={keys.join("+")} className={cn("inline-flex items-center gap-(--space-1)", className)}>
      {keys.map((key, index) => (
        <React.Fragment key={`${index}-${key}`}>
          {index > 0 ? (
            <span aria-hidden="true" className="text-caption text-(--text-muted)">
              +
            </span>
          ) : null}
          <kbd className="text-caption rounded-(--radius-control) border border-(--border) bg-(--subtle) px-(--space-2) py-px font-[family-name:var(--font-mono)] text-(--text)">
            {key}
          </kbd>
        </React.Fragment>
      ))}
    </span>
  );
}
