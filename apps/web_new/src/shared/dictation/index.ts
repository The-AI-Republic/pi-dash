// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
export {
  appendDictation,
  browserDictationDevices,
  DICTATION_MAX_MS,
  DICTATION_MIN_MS,
  DICTATION_TICK_MS,
  uploadDictationAudio,
  useDictation,
} from "./useDictation.js";
export type {
  DictationCaptureFailure,
  DictationDevices,
  DictationHandle,
  DictationStatus,
  DictationTranscriber,
  UseDictationOptions,
} from "./useDictation.js";
export { dictationOffered, sttConfigKeys, sttConfigQueryOptions, useDictationConfig } from "./config.js";
