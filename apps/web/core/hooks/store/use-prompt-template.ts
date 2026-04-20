/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useContext } from "react";
// mobx store
import { StoreContext } from "@/lib/store-context";
// types
import type { IPromptTemplateStore } from "@/store/prompt-template.store";

export const usePromptTemplate = (): IPromptTemplateStore => {
  const context = useContext(StoreContext);
  if (context === undefined) throw new Error("usePromptTemplate must be used within StoreProvider");
  return context.promptTemplate;
};
