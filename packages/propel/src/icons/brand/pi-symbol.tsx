/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import * as React from "react";

import type { ISvgIcons } from "../type";

export function PiSymbol({ width = "258", height = "140", className, color = "currentColor" }: ISvgIcons) {
  return (
    <svg
      width={width}
      height={height}
      viewBox="30 0 258 140"
      fill={color}
      xmlns="http://www.w3.org/2000/svg"
      className={className}
    >
      <g fill={color}>
        <circle cx="54" cy="70" r="24" />
        <rect x="84" y="46" width="72" height="48" rx="24" ry="24" />
        <rect x="162" y="46" width="72" height="48" rx="24" ry="24" />
        <circle cx="264" cy="70" r="24" />
      </g>
    </svg>
  );
}
