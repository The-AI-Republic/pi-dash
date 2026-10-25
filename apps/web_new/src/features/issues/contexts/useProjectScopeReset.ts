// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project-scoped resets for the archives tabs (parity/specs/archives.md
// §2.2/§2.5). The archived expression, display choices and peek selection
// all belong to one project; a project switch must not carry them over.

import * as React from "react";

/**
 * Runs `onScopeChange` once whenever the scope key changes, never on the
 * first mount. Callers pass the peek-clearing navigation; the first mount
 * is skipped because a reload of the same address must keep its peek.
 */
export function useProjectScopeReset(scope: string, onScopeChange: () => void): void {
  const previous = React.useRef(scope);
  const latest = React.useRef(onScopeChange);
  latest.current = onScopeChange;
  React.useEffect(() => {
    if (previous.current === scope) return;
    previous.current = scope;
    latest.current();
  }, [scope]);
}
