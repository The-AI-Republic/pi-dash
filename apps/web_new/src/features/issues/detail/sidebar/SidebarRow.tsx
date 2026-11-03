// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One property row in the detail sidebar (parity/specs/issues.md §13,
// ISS-148–152). Every row names its property on the left and renders a
// single control on the right; guests and archived items keep the row
// visible but hand the control a disabled state.

import * as React from "react";

export const SIDEBAR_ROW_CLASS = "flex min-h-8 items-center gap-(--space-3) py-px";
export const SIDEBAR_ROW_LABEL_CLASS = "w-24 shrink-0 text-meta text-(--text-muted)";
export const SIDEBAR_ROW_VALUE_CLASS = "min-w-0 flex-1 [&>button]:w-full [&>button]:justify-start";

export interface SidebarRowProps {
  /** Property name shown on the left. */
  label: string;
  /** The row's control, or a read-only value. */
  children: React.ReactNode;
}

export function SidebarRow({ label, children }: SidebarRowProps): React.ReactElement {
  const labelId = React.useId();
  return (
    <div className={SIDEBAR_ROW_CLASS} role="group" aria-labelledby={labelId}>
      <span id={labelId} className={SIDEBAR_ROW_LABEL_CLASS}>
        {label}
      </span>
      <div className={SIDEBAR_ROW_VALUE_CLASS}>{children}</div>
    </div>
  );
}
