// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The quiet frame the tab shows until its three reads land. It never
// shows a form with blank values: a seeded form appears only after the
// server's answer, so a member can never accidentally save over a stored
// key with empty fields.

import { Skeleton } from "@pidash/kit";
import * as React from "react";

export function AssistantSectionsSkeleton(): React.ReactElement {
  return (
    <div data-testid="assistant-settings-loading" aria-busy="true" className="flex max-w-2xl flex-col gap-(--space-8)">
      {[0, 1, 2].map((section) => (
        <div key={section} className="flex flex-col gap-(--space-3)">
          <Skeleton className="h-5 w-40" />
          <Skeleton className="h-4 w-full" />
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
        </div>
      ))}
    </div>
  );
}
