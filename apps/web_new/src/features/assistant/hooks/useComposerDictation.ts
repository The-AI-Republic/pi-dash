// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Composer dictation wiring (parity/specs/agents.md §12, AGT-050/051/064).
// One hook behind the landing and thread composers: the speech read plus
// the live capture handle, with the kill-switch refusal re-reading the
// config so the microphone hides itself.

import { useQueryClient } from "@tanstack/react-query";
import { dictationDisabledFrom } from "@pidash/api-client";
import * as React from "react";

import {
  sttConfigKeys,
  uploadDictationAudio,
  useDictation,
  useDictationConfig,
} from "../../../shared/dictation/index.js";
import type { ComposerDictation } from "../lib/dictation.js";

export function useComposerDictation(onResult: (text: string) => void): ComposerDictation {
  const speech = useDictationConfig();
  const queryClient = useQueryClient();
  const transcribe = React.useCallback(
    async (audio: Blob): Promise<string> => {
      try {
        return await uploadDictationAudio(audio);
      } catch (error) {
        if (dictationDisabledFrom(error) !== null) {
          void queryClient.invalidateQueries({ queryKey: sttConfigKeys.all() });
        }
        throw error;
      }
    },
    [queryClient]
  );
  const handle = useDictation({ onResult, transcribe });
  return { speech: speech.data, handle };
}
