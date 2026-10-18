// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Mutation loader state (parity/specs/issues.md §1, ISS-005). Filter and
// display writes run inside runWithMutationLoader so the floating spinner
// covers the write plus the list refetch it triggers; single-issue edits
// never enter, so they never raise the spinner.

import * as React from "react";

export interface IssuesLoader {
  /** True while at least one wrapped write is in flight. */
  isMutationLoading: boolean;
  /** Run a write with the mutation loader raised until it settles. */
  runWithMutationLoader: <T>(run: () => Promise<T>) => Promise<T>;
}

const IssuesLoaderContext = React.createContext<IssuesLoader | null>(null);

export function IssuesLoaderProvider({ children }: { children: React.ReactNode }): React.ReactElement {
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

/** Read the loader; every issues route renders under the provider. */
export function useIssuesLoader(): IssuesLoader {
  const loader = React.useContext(IssuesLoaderContext);
  if (loader === null) throw new Error("useIssuesLoader must be used under IssuesLoaderProvider.");
  return loader;
}

/**
 * Read the loader when one is mounted. Header builders can sit above the
 * layout root that owns the provider; their writes still apply and refetch,
 * they just cannot raise the layout's floating spinner.
 */
export function useOptionalIssuesLoader(): IssuesLoader | null {
  return React.useContext(IssuesLoaderContext);
}
