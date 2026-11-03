// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Route-facing entry for the calendar layout. The calendar's tiles, drag
// adapter and dialogs are a lazy split: the issues route is the size gate's
// first paint, and most visits open the list instead.

import * as React from "react";

import { CalendarSkeleton } from "../loaders/skeletons.js";
import type { CalendarLayoutProps } from "./CalendarLayout.js";

const LazyCalendarLayout = React.lazy(() =>
  import("./CalendarLayout.js").then((module) => ({ default: module.CalendarLayout }))
);

export function CalendarEntry(props: CalendarLayoutProps): React.ReactElement {
  return (
    <React.Suspense fallback={<CalendarSkeleton />}>
      <LazyCalendarLayout {...props} />
    </React.Suspense>
  );
}
