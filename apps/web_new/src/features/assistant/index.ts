// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the assistant feature (NEWFRONT-603). Other features import
// only from here; routes mount this feature's surfaces and take those
// directly from their modules, so a surface's dependencies never join the
// eager chunk of a feature that only needs the shared reads.

// The thread slice's modules (transport, transcript reads, send/stop
// writes, turn rules) are internal to this feature: the routes reach the
// conversation only through the gate, and keeping them off the barrel keeps
// them out of every eager importer's chunk.
export { assistantKeys } from "./api/keys.js";
export { START_FAILED_FALLBACK, startFailureDetail, useStartConversation } from "./api/mutations.js";
export {
  assistantConfigQueryOptions,
  assistantThreadsQueryOptions,
  useAssistantConfig,
  useAssistantThreads,
} from "./api/queries.js";
export {
  AGENT_LABEL,
  API_KEY_REMINDER,
  API_KEY_REQUIRED_TITLE,
  ASSISTANT_CAPTION,
  ASSISTANT_CARD_SUGGESTIONS,
  ASSISTANT_COMPOSER_LABEL,
  ASSISTANT_HEADLINE,
  ASSISTANT_MODEL_PICKER_LABEL,
  ASSISTANT_PLACEHOLDER,
  ASSISTANT_SEND_LABEL,
  ASSISTANT_STOP_LABEL,
  CHAT_HISTORY_TITLE,
  EMPTY_CONVERSATION_HINT,
  DESKTOP_ASSISTANT_LABEL,
  HANDOFF_FAILED_TITLE,
  lastModelRemoveBody,
  lastModelRemoveTitle,
  MODEL_ID_ADD_ACTION,
  MODEL_ID_ADD_PLACEHOLDER,
  MODEL_ID_CANCEL_REMOVE_ACTION,
  MODEL_ID_CONFIRM_REMOVE_ACTION,
  MODEL_ID_REMOVE_ACTION,
  MODEL_ID_STATUS_FAILED,
  MODEL_ID_STATUS_UNVERIFIED,
  MODEL_ID_STATUS_VERIFIED,
  MODEL_ID_TEST_ACTION,
  MODEL_ID_TESTING_LABEL,
  MODEL_IDS_HEADING,
  MODEL_IDS_SAVE_ANYWAY_ACTION,
  MODEL_PICKER_BLOCKED_BODY,
  MODEL_PICKER_BLOCKED_TITLE,
  MODEL_PICKER_FAILED_FALLBACK,
  MODEL_PICKER_FAILED_TITLE,
  MODELS_SECTION_HASH,
  NEW_CONVERSATION_LABEL,
  NO_CONVERSATIONS,
  providerSettingsHref,
  SETUP_CARD_ACTION,
  SETUP_CARD_BODY,
  SETUP_CARD_TITLE,
  START_FAILED_TITLE,
  SURFACE_MODEL_EMPTY_OPTION,
  UNTITLED_CONVERSATION,
  WEB_ASSISTANT_LABEL,
} from "./lib/copy.js";
export { isAssistantConfigured, needsLaneSetup } from "./lib/gate.js";
export { laneHasCredential, resolveLaneState } from "./lib/lane.js";
export type { LaneState } from "./lib/lane.js";
export { conversationLabel, visibleConversations } from "./lib/labels.js";
export { assistantLandingHref, assistantThreadHref, readPendingMessage } from "./lib/navigation.js";
export type { PendingMessageState } from "./lib/navigation.js";
export { OpenHubModelPicker } from "./components/OpenHubModelPicker.js";
export type { OpenHubModelPickerProps } from "./components/OpenHubModelPicker.js";
export { SurfaceModelPickers, surfacePickerLabel } from "./components/SurfaceModelPickers.js";
export type { SurfaceModelPickersProps, SurfacePickerSpec } from "./components/SurfaceModelPickers.js";
export { ModelIdEditor } from "./components/ModelIdEditor.js";
export type { ModelIdEditorProps } from "./components/ModelIdEditor.js";
export {
  modelFailedIds,
  modelFailureDetails,
  modelIdStatus,
  normalizeModelIds,
  saveAnywayModels,
} from "./lib/byokModels.js";
export type { ModelIdStatus } from "./lib/byokModels.js";
export {
  SELECTION_FAILED_FALLBACK,
  composerSurface,
  modelSelectionActions,
  modelSelectionQueryOptions,
  selectionFailureDetail,
  useModelSelection,
  useModelSelectionActions,
} from "./api/selection.js";
export type { ModelSelectionWrites } from "./api/selection.js";
export {
  OPENHUB_DEFAULT_MODEL,
  OPENHUB_DEFAULT_SUFFIX,
  applyCreditBadges,
  buildMergedModelChoices,
  byokChoiceLabel,
  byokKeyFromConfig,
  byokKeysFromList,
  catalogFromList,
  effectiveChoiceFor,
  fallbackNotice,
  mergedChoiceKey,
  pickerOptionLabel,
} from "./lib/openhubPicker.js";
export type { MergedPickerByokKey, MergedPickerCatalog, MergedPickerChoice } from "./lib/openhubPicker.js";
