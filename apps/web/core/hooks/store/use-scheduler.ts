/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useContext } from "react";
// mobx store
import { StoreContext } from "@/lib/store-context";
// types
import type { ISchedulerStore } from "@/store/scheduler.store";

export const useScheduler = (): ISchedulerStore => {
  const context = useContext(StoreContext);
  if (context === undefined) throw new Error("useScheduler must be used within StoreProvider");
  return context.scheduler;
};
