// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// First-load placeholder for the pages collection (parity/specs/pages.md
// §3.7, PAGE-019): a header line plus row placeholders that mirror the
// mark, title, owner and menu anatomy.

import { Skeleton } from "@pidash/kit";
import * as React from "react";

export function PagesListSkeleton(): React.ReactElement {
  return (
    <div data-testid="pages-list-skeleton" aria-busy="true" className="flex flex-col gap-(--space-2)">
      <Skeleton className="h-7 w-48" />
      {[0, 1, 2, 3].map((index) => (
        <div key={index} className="flex items-center gap-(--space-3) px-(--space-4) py-(--space-3)">
          <Skeleton className="size-6 rounded-(--radius-control)" />
          <Skeleton className="h-4 flex-1" />
          <Skeleton className="size-6 rounded-(--radius-full)" />
          <Skeleton className="size-5 rounded-(--radius-control)" />
        </div>
      ))}
    </div>
  );
}
