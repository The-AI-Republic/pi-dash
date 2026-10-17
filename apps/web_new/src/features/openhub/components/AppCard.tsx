// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Catalog app card, shell pass (CLOUD-022/023, from
// parity/specs/cloud.md §5). This slice renders the identity S6 builds
// on — install/credential badges, actions and icon fallbacks arrive
// with the cards slice (CLOUD-028, NEWFRONT-659).
import type { OpenhubApp } from "@pidash/api-client";
import * as React from "react";

export function AppCard({ app }: { app: OpenhubApp }): React.ReactElement {
  return (
    <li data-testid="app-card" data-app-id={app.appId}>
      <h3 data-testid="app-card-name">{app.name}</h3>
    </li>
  );
}
