// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runners area context (NEWFRONT-540). The scope shells publish the resolved
// scope and access decision here so child routes read one source instead of
// re-deriving them. Downstream slices (runs, approvals, detail, chat) use
// `useRunnersArea()` for the same scope object and base helpers.

import * as React from "react";

import type { RunnersAccess } from "./access.js";
import { runnersBase, runnersPath, type RunnersScopeInput } from "./scope.js";

export interface RunnersAreaValue {
  scope: RunnersScopeInput;
  workspaceId: string;
  access: RunnersAccess;
  /** True once access is allowed; child screens gate their reads on it. */
  allowed: boolean;
  /** `/ws/runners` or `/ws/projects/<id>/runners`. */
  base: string;
  /** A child path below the base. */
  path: (child?: string) => string;
}

const RunnersAreaContext = React.createContext<RunnersAreaValue | null>(null);

/** Props for the provider inside the scope shells. */
export function buildRunnersAreaValue(
  scope: RunnersScopeInput,
  workspaceId: string,
  access: RunnersAccess
): RunnersAreaValue {
  return {
    scope,
    workspaceId,
    access,
    allowed: access === "allowed",
    base: runnersBase(scope),
    path: (child?: string) => runnersPath(scope, child),
  };
}

export function RunnersAreaProvider({
  value,
  children,
}: {
  value: RunnersAreaValue;
  children: React.ReactNode;
}): React.ReactElement {
  return <RunnersAreaContext.Provider value={value}>{children}</RunnersAreaContext.Provider>;
}

/** The enclosing runners area; throws outside a runners route. */
export function useRunnersArea(): RunnersAreaValue {
  const value = React.useContext(RunnersAreaContext);
  if (value === null) throw new Error("useRunnersArea must be used inside a runners route");
  return value;
}
