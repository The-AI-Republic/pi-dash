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
  API_KEY_REMINDER,
  API_KEY_REQUIRED_TITLE,
  ASSISTANT_CAPTION,
  ASSISTANT_CARD_SUGGESTIONS,
  ASSISTANT_COMPOSER_LABEL,
  ASSISTANT_HEADLINE,
  ASSISTANT_PLACEHOLDER,
  ASSISTANT_SEND_LABEL,
  ASSISTANT_STOP_LABEL,
  CONVERSATION_RAIL_TITLE,
  EMPTY_CONVERSATION_HINT,
  HANDOFF_FAILED_TITLE,
  NEW_CONVERSATION_LABEL,
  NO_CONVERSATIONS,
  providerSettingsHref,
  SETUP_CARD_ACTION,
  SETUP_CARD_BODY,
  SETUP_CARD_TITLE,
  START_FAILED_TITLE,
  UNTITLED_CONVERSATION,
} from "./lib/copy.js";
export { isAssistantConfigured, needsLaneSetup } from "./lib/gate.js";
export { laneHasCredential, resolveLaneState } from "./lib/lane.js";
export type { LaneState } from "./lib/lane.js";
export { conversationLabel, visibleConversations } from "./lib/labels.js";
export { assistantLandingHref, assistantThreadHref, readPendingMessage } from "./lib/navigation.js";
export type { PendingMessageState } from "./lib/navigation.js";
