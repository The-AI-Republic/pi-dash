/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import * as React from "react";
import { TimelineLayoutIcon, GridLayoutIcon, ListLayoutIcon } from "@apple-pi-dash/propel/icons";
import type { TModuleLayoutOptions } from "@apple-pi-dash/types";
import { cn } from "@apple-pi-dash/utils";

interface ILayoutIcon {
  className?: string;
  containerClassName?: string;
  layoutType: TModuleLayoutOptions;
  size?: number;
  withContainer?: boolean;
}

export function ModuleLayoutIcon(props: ILayoutIcon) {
  const { layoutType, className = "", containerClassName = "", size = 14, withContainer = false } = props;

  // get Layout icon
  const icons = {
    list: ListLayoutIcon,
    board: GridLayoutIcon,
    gantt: TimelineLayoutIcon,
  };
  const Icon = icons[layoutType ?? "list"];

  if (!Icon) return null;

  return (
    <>
      {withContainer ? (
        <div
          className={cn("flex flex-shrink-0 items-center justify-center rounded-sm border p-0.5", containerClassName)}
        >
          <Icon width={size} height={size} className={cn(className)} />
        </div>
      ) : (
        <Icon width={size} height={size} className={cn("flex-shrink-0", className)} />
      )}
    </>
  );
}
