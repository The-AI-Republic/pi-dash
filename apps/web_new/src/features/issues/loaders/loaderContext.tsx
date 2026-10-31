// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Mutation loader state (parity/specs/issues.md §1, ISS-005). Filter and
// display writes run inside runWithMutationLoader so the floating spinner
// covers the write plus the list refetch it triggers; single-issue edits
// never enter, so they never raise the spinner. Whoever builds the
// elements that issue those writes owns the scope, because a control
// mounted above a layout root still has to reach the same loader the
// root draws from. A nested provider reuses an outer one so one screen
// never splits its writes across two counters.

import * as React from "react";

export interface IssuesLoader {
  /** True while at least one wrapped write is in flight. */
  isMutationLoading: boolean;
  /** Run a write with the mutation loader raised until it settles. */
  runWithMutationLoader: <T>(run: () => Promise<T>) => Promise<T>;
}

const IssuesLoaderContext = React.createContext<IssuesLoader | null>(null);

export function IssuesLoaderProvider({ children }: { children: React.ReactNode }): React.ReactElement {
  const outer = React.useContext(IssuesLoaderContext);
  return outer === null ? <IssuesLoaderScope>{children}</IssuesLoaderScope> : <>{children}</>;
}

function IssuesLoaderScope({ children }: { children: React.ReactNode }): React.ReactElement {
  const [inflight, setInflight] = React.useState(0);
  const runWithMutationLoader: IssuesLoader["runWithMutationLoader"] = React.useCallback(async (run) => {
    setInflight((count) => count + 1);
    try {
      return await run();
    } finally {
      setInflight((count) => count - 1);
    }
  }, []);
  const value = React.useMemo<IssuesLoader>(
    () => ({ isMutationLoading: inflight > 0, runWithMutationLoader }),
    [inflight, runWithMutationLoader]
  );
  return <IssuesLoaderContext.Provider value={value}>{children}</IssuesLoaderContext.Provider>;
}

/** Read the loader; every issues route renders under a provider scope. */
export function useIssuesLoader(): IssuesLoader {
  const loader = React.useContext(IssuesLoaderContext);
  if (loader === null) throw new Error("useIssuesLoader must be used under IssuesLoaderProvider.");
  return loader;
}

/** Read the loader when a scope is mounted; temporary rows write none at all. */
export function useOptionalIssuesLoader(): IssuesLoader | null {
  return React.useContext(IssuesLoaderContext);
}
