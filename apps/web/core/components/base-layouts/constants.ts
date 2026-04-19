/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { BoardLayoutIcon, ListLayoutIcon, TimelineLayoutIcon } from "@pi-dash/propel/icons";
import type { IBaseLayoutConfig } from "@pi-dash/types";

export const BASE_LAYOUTS: IBaseLayoutConfig[] = [
  {
    key: "list",
    icon: ListLayoutIcon,
    label: "List Layout",
  },
  {
    key: "kanban",
    icon: BoardLayoutIcon,
    label: "Board Layout",
  },
  {
    key: "gantt",
    icon: TimelineLayoutIcon,
    label: "Gantt Layout",
  },
];
