// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runners area shell (NEWFRONT-540, RUN-001/RUN-004). Composes the role
// gate, the side rail and the content column that nested routes mount
// into. The gate waits for the role records: nothing renders meanwhile, so
// a viewer never sees the denial or a half-built area first. Denied viewers
// still get area chrome without the rail; the route layouts supply the
// surrounding workspace or project frame.
//
// Downstream slices mount their screens as children: overview (this
// slice), runs, approvals, runner detail and chat.

import * as React from "react";

import type { Runner } from "@pidash/api-client";

import type { LocalContact } from "../chat/localContact.js";
import type { RunnersAccess } from "../access.js";
import type { RunnersScopeInput } from "../scope.js";
import { RunnersNotAuthorized } from "./NotAuthorizedView.js";
import { RunnersRail } from "./RunnersRail.js";
import { RunnersTabStrip } from "./RunnersTabStrip.js";

export interface RunnersAreaShellProps {
  scope: RunnersScopeInput;
  access: RunnersAccess;
  /** The rail's runners; empty until the list lands. */
  runners: readonly Runner[];
  pathname: string;
  /** Project chrome (breadcrumb header) rendered above the area. */
  chrome?: React.ReactNode;
  /** The built-in local contact; absent on cloud and web (RUN-033). */
  localContacts?: readonly LocalContact[];
  children: React.ReactNode;
}

export function isProjectScope(scope: RunnersScopeInput): boolean {
  return scope.projectId !== undefined;
}

export function RunnersAreaShell({
  scope,
  access,
  runners,
  pathname,
  chrome,
  localContacts,
  children,
}: RunnersAreaShellProps): React.ReactElement | null {
  if (access === "loading") return null;
  if (access === "denied") {
    return (
      <>
        {chrome}
        <RunnersNotAuthorized scope={isProjectScope(scope) ? "project" : "workspace"} />
      </>
    );
  }
  return (
    <>
      {chrome}
      <div className="flex min-h-0 w-full flex-1 overflow-hidden">
        <RunnersRail
          scope={scope}
          runners={runners}
          isProjectScope={isProjectScope(scope)}
          {...(localContacts === undefined ? {} : { localContacts })}
        />
        <main aria-label="AI Workers content" data-testid="runners-content" className="min-w-0 flex-1 overflow-auto">
          <RunnersTabStrip scope={scope} pathname={pathname} />
          {children}
        </main>
      </div>
    </>
  );
}
