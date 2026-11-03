// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The module status mark for palette rows (parity/specs/modules.md §4.2,
// MOD-037). A small filled dot in the module's own status color, drawn
// for decoration: the row's text already names the status, so the dot is
// hidden from assistive technology.

import * as React from "react";

export interface ModuleStatusDotProps {
  status: string;
  /** The status' color token value. */
  color: string;
}

export function ModuleStatusDot({ status, color }: ModuleStatusDotProps): React.ReactElement {
  return (
    <svg
      width="10"
      height="10"
      viewBox="0 0 10 10"
      aria-hidden="true"
      focusable="false"
      data-testid="module-status-dot"
      data-module-status={status}
      className="shrink-0"
    >
      <circle cx="5" cy="5" r="4" fill={color} />
    </svg>
  );
}
