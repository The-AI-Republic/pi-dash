// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the assistant feature (NEWFRONT-603). Other features and
// routes import only from here.

export { assistantKeys } from "./api/keys.js";
export {
  START_FAILED_FALLBACK,
  startFailureDetail,
  useDeliverFirstMessage,
  useStartConversation,
} from "./api/mutations.js";
export {
  assistantConfigQueryOptions,
  assistantThreadsQueryOptions,
  useAssistantConfig,
  useAssistantThreads,
} from "./api/queries.js";
export { AssistantLanding } from "./components/AssistantLanding.js";
export type { AssistantLandingProps } from "./components/AssistantLanding.js";
export { AssistantLayout } from "./components/AssistantLayout.js";
export type { AssistantLayoutProps } from "./components/AssistantLayout.js";
export { Composer } from "./components/Composer.js";
export type { ComposerProps } from "./components/Composer.js";
export { ConversationGate } from "./components/ConversationGate.js";
export type { ConversationGateProps } from "./components/ConversationGate.js";
export { ConversationRail } from "./components/ConversationRail.js";
export type { ConversationRailProps } from "./components/ConversationRail.js";
export { SetupCard } from "./components/SetupCard.js";
export type { SetupCardProps } from "./components/SetupCard.js";
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
  PROVIDER_SETTINGS_HREF,
  SETUP_CARD_ACTION,
  SETUP_CARD_BODY,
  SETUP_CARD_TITLE,
  START_FAILED_TITLE,
  UNTITLED_CONVERSATION,
} from "./lib/copy.js";
export { isAssistantConfigured, needsLaneSetup, needsSetup } from "./lib/gate.js";
export { laneHasCredential, resolveLaneState } from "./lib/lane.js";
export type { LaneState } from "./lib/lane.js";
export { conversationLabel, visibleConversations } from "./lib/labels.js";
export { assistantLandingHref, assistantThreadHref, readPendingMessage } from "./lib/navigation.js";
export type { PendingMessageState } from "./lib/navigation.js";
