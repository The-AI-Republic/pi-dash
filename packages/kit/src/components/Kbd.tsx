// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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
