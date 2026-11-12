// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Catalog app card (written from parity/specs/cloud.md §6,
// CLOUD-028): the installed and unfinished markers, state-driven
// actions behind a per-card busy guard, and the self-hosted-graphic
// to proxy to letter-tile icon chain.
import type { OpenhubApp } from "@pidash/api-client";
import { Badge, Button } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import {
  appCardActions,
  appCardBadge,
  appIconSource,
  appLetter,
  firstIconStage,
  nextIconStage,
  type AppCardAction,
  type AppIconStage,
} from "../lib/cards.js";

export const APP_INSTALLED_BADGE = "Installed";
export const APP_NEEDS_CONNECTION_BADGE = "Needs connection";

export interface AppCardProps {
  app: OpenhubApp;
  onInstall: (app: OpenhubApp) => Promise<void>;
  onConnect: (app: OpenhubApp) => Promise<void>;
  onUninstall: (app: OpenhubApp) => Promise<void>;
}

export function AppCard({ app, onInstall, onConnect, onUninstall }: AppCardProps): React.ReactElement {
  const [busy, setBusy] = React.useState(false);
  const [icon, setIcon] = React.useState(() => ({ forApp: app.appId, stage: firstIconStage(app) }));
  // Same slot, new row: restart the chain (the pager's render-time
  // reset pattern — the effect-free way to follow a prop change).
  const stage: AppIconStage = icon.forApp === app.appId ? icon.stage : firstIconStage(app);
  if (icon.forApp !== app.appId) {
    setIcon({ forApp: app.appId, stage });
  }
  const badge = appCardBadge(app);
  const actions = appCardActions(app);
  const handlers: Record<AppCardAction, (target: OpenhubApp) => Promise<void>> = {
    Install: onInstall,
    Connect: onConnect,
    Uninstall: onUninstall,
  };
  const iconSource = appIconSource(app, stage, getAppClient().baseUrl);

  const run = (action: (target: OpenhubApp) => Promise<void>) => (): void => {
    if (busy) return;
    setBusy(true);
    void (async () => {
      try {
        await action(app);
      } finally {
        setBusy(false);
      }
    })();
  };

  return (
    <li data-testid="app-card" data-app-id={app.appId} className="flex items-center gap-(--space-4)">
      {iconSource === null ? (
        <span
          aria-hidden="true"
          data-testid="app-card-tile"
          className="text-meta inline-flex size-8 shrink-0 items-center justify-center rounded-(--radius-full) bg-(--subtle) font-medium text-(--text-muted) select-none"
        >
          {appLetter(app.name)}
        </span>
      ) : (
        <img
          src={iconSource}
          alt=""
          data-testid="app-card-icon"
          className="size-8 shrink-0 rounded-(--radius-full) object-cover"
          onError={() => setIcon({ forApp: app.appId, stage: nextIconStage(app, stage) })}
        />
      )}
      <div className="flex min-w-0 flex-1 flex-col items-start gap-(--space-1)">
        <h3 data-testid="app-card-name" className="text-emphasis text-(--text)">
          {app.name}
        </h3>
        {app.description === undefined || app.description === "" ? null : (
          <p className="text-body text-(--text-muted)">{app.description}</p>
        )}
        {badge === null ? null : (
          <Badge tone={badge === "installed" ? "success" : "warning"} dot>
            {badge === "installed" ? APP_INSTALLED_BADGE : APP_NEEDS_CONNECTION_BADGE}
          </Badge>
        )}
      </div>
      <div className="flex shrink-0 gap-(--space-2)">
        {actions.map((action) => (
          <Button key={action} loading={busy} onClick={run(handlers[action])}>
            {action}
          </Button>
        ))}
      </div>
    </li>
  );
}
