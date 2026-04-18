/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { ReactNode } from "react";
// helpers
import { cn } from "@pi-dash/utils";

export interface ContentWrapperProps {
  className?: string;
  children: ReactNode;
}

export function ContentWrapper({ className, children }: ContentWrapperProps) {
  return (
    <div className="h-full w-full overflow-hidden">
      <div className={cn("relative h-full w-full overflow-x-hidden overflow-y-scroll", className)}>{children}</div>
    </div>
  );
}
