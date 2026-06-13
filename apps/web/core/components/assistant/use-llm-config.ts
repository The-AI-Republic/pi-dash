/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import useSWR from "swr";
import { AssistantService } from "@pi-dash/services";
import type { IUserLLMConfig } from "@pi-dash/types";

const service = new AssistantService();

/** Shared BYOK config state for the assistant entry points. */
export function useLLMConfig(enabled = true) {
  const { data: config } = useSWR<IUserLLMConfig>(enabled ? "assistant-llm-config" : null, () =>
    service.getLLMConfig()
  );
  // `config` is undefined while loading — only treat as "needs setup" once known.
  const needsSetup = config?.has_api_key === false;
  return { config, needsSetup };
}
