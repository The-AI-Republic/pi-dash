/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { createContext, useContext } from "react";
import { useParams } from "next/navigation";

/**
 * The workspace "global view" issues UI normally reads the active view id from
 * the `:globalViewId` route param (e.g. `/workspace-views/all-issues`). The
 * shorter `/all-issues` route has no such param, so this context lets the route
 * pin a static view id (e.g. `"all-issues"`) for the subtree below it.
 *
 * Consumers should call {@link useGlobalViewId}, which prefers the context value
 * and falls back to the route param so both URL shapes keep working.
 */
const GlobalViewIdContext = createContext<string | undefined>(undefined);

export function GlobalViewIdProvider(props: { value: string; children: React.ReactNode }) {
  return <GlobalViewIdContext.Provider value={props.value}>{props.children}</GlobalViewIdContext.Provider>;
}

export function useGlobalViewId(): string | undefined {
  const contextValue = useContext(GlobalViewIdContext);
  const { globalViewId: routerGlobalViewId } = useParams();
  return contextValue ?? (routerGlobalViewId ? routerGlobalViewId.toString() : undefined);
}
