/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useContext } from "react";
// types
import type { TTimelineType } from "@apple-pi-dash/types";
// lib
import { StoreContext } from "@/lib/store-context";
// Apple Pi Dash-web
import { getTimelineStore } from "@/apple-pi-dash-web/hooks/use-timeline-chart";
import type { IBaseTimelineStore } from "@/apple-pi-dash-web/store/timeline/base-timeline.store";
import { useTimeLineType } from "../components/gantt-chart/contexts";

export const useTimeLineChart = (timelineType: TTimelineType): IBaseTimelineStore => {
  const context = useContext(StoreContext);
  if (!context) throw new Error("useTimeLineChart must be used within StoreProvider");

  return getTimelineStore(context.timelineStore, timelineType);
};

export const useTimeLineChartStore = (): IBaseTimelineStore => {
  const context = useContext(StoreContext);
  const timelineType = useTimeLineType();

  if (!context) throw new Error("useTimeLineChartStore must be used within StoreProvider");
  if (!timelineType) throw new Error("useTimeLineChartStore must be used within TimeLineTypeContext");

  return getTimelineStore(context.timelineStore, timelineType);
};
