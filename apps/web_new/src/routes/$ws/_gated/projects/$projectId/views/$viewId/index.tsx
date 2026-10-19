// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved project view route (parity/specs/issues.md §21, ISS-104). Thin so
// the router's eager import of this file stays cheap: the search schema is
// shared with the issues screen, the record and list reads run on visit in
// the preload module, and the screen itself, its stored-expression modules
// and the filter row ride a lazy chunk (Quality gates: initial JS). The
// views area owns the full views surface and replaces this host when it
// lands.

import { createFileRoute } from "@tanstack/react-router";
import { Button } from "@pidash/kit";
import * as React from "react";

import { isProjectGateError } from "../../../../../../../features/auth/index.js";
import { ListSkeleton, parseIssueSearch } from "../../../../../../../features/issues/index.js";
import type { SavedViewReadInput } from "../../../../../../../features/issues/filters/rich/savedViewPreload.js";
import { projectsQueryOptions } from "../../../../../../../features/projects/index.js";

type ScreenModule = typeof import("./SavedViewScreen.js");

let request: Promise<ScreenModule> | null = null;

// Start the screen chunk with the route module: `beforeLoad` awaits the
// same request beside the view's reads, so the visit does not serialize a
// round trip behind them. A failure is left for `beforeLoad` to report,
// which turns it into the route's load error.
function startSavedView(): Promise<ScreenModule> {
  request ??= import("./SavedViewScreen.js");
  return request;
}

void startSavedView().catch(() => undefined);

// The body still goes through a lazy boundary: it is the same cached
// request, so a visit that ran `beforeLoad` resolves without a second
// fetch, and a direct render (tests, an unexpected entry) shows the route
// skeleton instead of a blank shell.
const LazySavedViewScreen = React.lazy(() => startSavedView().then((module) => ({ default: module.SavedViewScreen })));

function SavedViewRouteComponent(): React.ReactElement {
  const params = Route.useParams() as SavedViewReadInput;
  const search = Route.useSearch();
  const context = Route.useRouteContext();
  return (
    <React.Suspense fallback={<ListSkeleton />}>
      <LazySavedViewScreen params={params} search={search} context={context} />
    </React.Suspense>
  );
}

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/views/$viewId/")({
  validateSearch: (search: Record<string, unknown>) => parseIssueSearch(search),
  beforeLoad: async ({ context, params, search }) => {
    try {
      // The screen chunk also carries the view's reads, so the first
      // commit is the screen with its data.
      const { prefetchSavedView } = await startSavedView();
      await prefetchSavedView(context.queryClient, context.apiClient, params, search);
    } catch (error) {
      if (!isProjectGateError(error)) throw error;
    }
  },
  loader: async ({ context, params }) => {
    await context.queryClient.ensureQueryData(projectsQueryOptions(context.apiClient, params.ws));
  },
  pendingComponent: function ViewIssuesPending() {
    return <ListSkeleton />;
  },
  errorComponent: function ViewIssuesError({ error, reset }: { error: unknown; reset: () => void }) {
    return (
      <section aria-label="Issues" className="flex flex-col items-center gap-(--space-4) p-(--space-12)">
        <h2 className="text-h3 text-(--text)">Could not load issues</h2>
        <p className="text-body text-(--text-muted)">
          {error instanceof Error ? error.message : "Something went wrong."}
        </p>
        <Button variant="primary" onClick={reset}>
          Try again
        </Button>
      </section>
    );
  },
  component: SavedViewRouteComponent,
});
