/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import useSWR from "swr";
import { AssistantService } from "@pi-dash/services";
import type { IUserSTTConfig } from "@pi-dash/types";

const service = new AssistantService();

/**
 * The user's speech-to-text (dictation) config, shared through one SWR key by
 * the composer mic, the AI Assistant settings page and the dictation section.
 * `data.enabled` is the instance `VOICE_DICTATION_ENABLED` kill switch; `data`
 * is undefined while loading or when the read fails, which callers treat as
 * disabled.
 */
export function useSTTConfig() {
  return useSWR<IUserSTTConfig>("assistant-stt-config", () => service.getSTTConfig(), {
    shouldRetryOnError: false,
  });
}
