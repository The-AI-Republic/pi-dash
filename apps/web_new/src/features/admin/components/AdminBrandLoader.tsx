// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Brand loader (ADM-002, from parity/specs/admin.md §1.2): a centered
// pulsing spinner covering the full screen while the entry gate is
// unresolved. It never renders beside a form or the shell.

import { Spinner } from "@pidash/kit";
import * as React from "react";

export function AdminBrandLoader(): React.ReactElement {
  return (
    <main
      data-testid="admin-loader"
      className="flex min-h-screen items-center justify-center bg-(--bg)"
      aria-label="Loading the admin console"
    >
      <span className="animate-pulse text-(--text-muted)">
        <Spinner size="large" label="Loading the admin console" />
      </span>
    </main>
  );
}
