/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import {
  ListLayoutIcon,
  BoardLayoutIcon,
  CalendarLayoutIcon,
  SheetLayoutIcon,
  TimelineLayoutIcon,
} from "@apple-pi-dash/propel/icons";
import type { ISvgIcons } from "@apple-pi-dash/propel/icons";
import { EIssueLayoutTypes } from "@apple-pi-dash/types";

export function IssueLayoutIcon({
  layout,
  size,
  ...props
}: { layout: EIssueLayoutTypes; size?: number } & Omit<ISvgIcons, "width" | "height">) {
  const iconProps = {
    ...props,
    ...(size && { width: size, height: size }),
  };

  switch (layout) {
    case EIssueLayoutTypes.LIST:
      return <ListLayoutIcon {...iconProps} />;
    case EIssueLayoutTypes.KANBAN:
      return <BoardLayoutIcon {...iconProps} />;
    case EIssueLayoutTypes.CALENDAR:
      return <CalendarLayoutIcon {...iconProps} />;
    case EIssueLayoutTypes.SPREADSHEET:
      return <SheetLayoutIcon {...iconProps} />;
    case EIssueLayoutTypes.GANTT:
      return <TimelineLayoutIcon {...iconProps} />;
    default:
      return null;
  }
}
