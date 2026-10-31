// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Connected catalog (written from parity/specs/cloud.md §5,
// CLOUD-022/023/029/030, and §6, CLOUD-024/027/028): viewer-ownership
// intro, debounced search, agent-tools switch, card grid, cursor
// pager, the inline catalog states, and the install/connect/remove
// card flows with the credential dialog. Mounted only behind a
// connected status read (AppsView gates).
import { Button, EmptyState, Input, Spinner } from "@pidash/kit";
import { Blocks, SearchX } from "lucide-react";
import * as React from "react";

import { useDebouncedValue } from "../../../shared/commands/index.js";
import { useAppCardActions } from "../api/actions.js";
import { useAgentToolsEnabled, useSetAgentToolsEnabled } from "../api/mutations.js";
import { APPS_SEARCH_DEBOUNCE_MS, useMarketplacePage } from "../api/queries.js";
import { catalogFailureMessage } from "../lib/errors.js";
import { useCatalogPager } from "../lib/pager.js";
import { AppCard } from "./AppCard.js";
import { AppsToggle } from "./AppsToggle.js";
import { ConnectDialog } from "./ConnectDialog.js";

/**
 * Viewer-ownership intro. The oracle pins the yours/teammates wording
 * (apps-shell.spec.ts): two viewers on one URL see different lists.
 */
export const APPS_CATALOG_INTRO =
  "Apps you install are yours: they extend your assistant automatically, " +
  "and teammates opening this page see their own apps instead.";

export const APPS_SEARCH_LABEL = "Search";
export const APPS_SEARCH_PLACEHOLDER = "Search apps";

export const APPS_EMPTY_TITLE = "No apps available";
export const APPS_EMPTY_DESCRIPTION = "There are no apps in the catalog yet. Check back later.";
export const APPS_NO_MATCHES_TITLE = "No matching apps";

/** No-matches line, echoing the query (CLOUD-030). */
export function appsNoMatchesDescription(query: string): string {
  return `No apps match "${query}". Try a different search.`;
}

export function AppsCatalog({ workspaceSlug }: { workspaceSlug: string }): React.ReactElement {
  const [draft, setDraft] = React.useState("");
  const settled = useDebouncedValue(draft, APPS_SEARCH_DEBOUNCE_MS).trim();
  const pager = useCatalogPager(settled);
  const marketplace = useMarketplacePage(workspaceSlug, { query: settled, cursor: pager.cursor });
  const agentTools = useAgentToolsEnabled();
  const setAgentTools = useSetAgentToolsEnabled();
  const cardActions = useAppCardActions(workspaceSlug);
  const dialog = cardActions.dialog;
  const nextCursor = marketplace.data?.nextCursor ?? null;
  const pagerVisible = pager.page > 1 || nextCursor !== null;

  return (
    <div className="flex flex-col gap-(--space-4)">
      <h1 className="text-h2 text-(--text)">Apps</h1>
      <p className="text-body text-(--text-muted)">{APPS_CATALOG_INTRO}</p>
      <Input
        label={APPS_SEARCH_LABEL}
        type="search"
        placeholder={APPS_SEARCH_PLACEHOLDER}
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
      />
      {agentTools === undefined ? null : (
        <AppsToggle
          enabled={agentTools}
          disabled={setAgentTools.isPending}
          onToggle={(next) => setAgentTools.mutate(next)}
        />
      )}
      {marketplace.isError ? (
        <div role="alert" className="flex flex-col items-start gap-(--space-3)">
          <p className="text-body text-(--text)">{catalogFailureMessage(marketplace.error)}</p>
          <Button onClick={() => void marketplace.refetch()}>Retry</Button>
        </div>
      ) : marketplace.data === undefined ? (
        <Spinner label="Loading apps" />
      ) : marketplace.data.items.length === 0 ? (
        settled === "" ? (
          <EmptyState
            variant="section"
            title={APPS_EMPTY_TITLE}
            description={APPS_EMPTY_DESCRIPTION}
            icon={<Blocks size={20} strokeWidth={1.5} aria-hidden="true" />}
          />
        ) : (
          <EmptyState
            variant="section"
            title={APPS_NO_MATCHES_TITLE}
            description={appsNoMatchesDescription(settled)}
            icon={<SearchX size={20} strokeWidth={1.5} aria-hidden="true" />}
          />
        )
      ) : (
        <>
          <ul data-testid="apps-catalog-grid" aria-label="Apps" aria-busy={marketplace.isFetching}>
            {marketplace.data.items.map((app) => (
              <AppCard
                key={app.appId}
                app={app}
                onInstall={cardActions.install}
                onConnect={cardActions.connect}
                onUninstall={cardActions.remove}
              />
            ))}
          </ul>
          {pagerVisible ? (
            <nav aria-label="Catalog pages" className="flex items-center gap-(--space-3)">
              <Button onClick={pager.goPrevious} disabled={pager.page === 1 || marketplace.isFetching}>
                Previous
              </Button>
              <p data-testid="apps-page-indicator" className="text-body text-(--text-muted)">
                Page {pager.page}
              </p>
              <Button onClick={() => pager.goNext(nextCursor)} disabled={nextCursor === null || marketplace.isFetching}>
                Next
              </Button>
            </nav>
          ) : null}
        </>
      )}
      {dialog === null ? null : (
        <ConnectDialog
          app={dialog.app}
          auth={dialog.auth}
          onSubmit={(fields) => cardActions.submitCredentials(dialog.app, fields)}
          onClose={cardActions.closeDialog}
        />
      )}
    </div>
  );
}
