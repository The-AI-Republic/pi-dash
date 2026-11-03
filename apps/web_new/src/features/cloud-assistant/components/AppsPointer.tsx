// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Apps pointer (parity/specs/cloud.md §7, CLOUD-036). The catalog is a
// browsable, searchable page under the workspace's Apps entry; duplicating
// it inside a settings tab would give it two homes. The pointer names that
// destination and carries no controls of its own.

import * as React from "react";

import { APPS_POINTER } from "../lib/copy.js";

export function AppsPointer(): React.ReactElement {
  return (
    <p className="text-body text-(--text-muted)" data-testid="cloud-assistant-apps-pointer">
      {APPS_POINTER}
    </p>
  );
}
