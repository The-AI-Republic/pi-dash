// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the cloud assistant feature (NEWFRONT-660). Other features
// and routes import only from here.

export { cloudAssistantKeys } from "./api/keys.js";
export { actionFailureDetail, cloudAssistantActions, useCloudAssistantActions } from "./api/mutations.js";
export type { CloudAssistantActions } from "./api/mutations.js";
export {
  USAGE_QUOTA_STALE_TIME_MS,
  cloudAssistantConfigQueryOptions,
  cloudConnectionsQueryOptions,
  cloudQuotaQueryOptions,
  useCloudAssistantConfig,
  useCloudConnections,
  useCloudQuota,
  usageQuotaQueryOptions,
  useFreeTokenUsage,
} from "./api/queries.js";
export { AppsPointer } from "./components/AppsPointer.js";
export { CloudAssistantTab } from "./components/CloudAssistantTab.js";
export { FreeTokenMeter } from "./components/FreeTokenMeter.js";
export { useProviderSignIn } from "./components/useProviderSignIn.js";
export type { ProviderSignIn, ProviderSignInOptions } from "./components/useProviderSignIn.js";
export {
  BYOK_PROVIDERS,
  EMPTY_BYOK_DRAFT,
  byokDraftValid,
  byokSubmission,
  needsBaseUrl,
  validateByokDraft,
} from "./lib/byok.js";
export type { ByokDraft, ByokProblems, ByokProvider, ByokSubmission } from "./lib/byok.js";
export {
  BUILTIN_CODEX_MODELS,
  BYOK_MODEL_SUGGESTIONS,
  connectionModelFlags,
  connectionTitle,
  creditForModel,
  isBlockedCommit,
  isDisconnected,
  isStaleHostedRow,
  modelChoices,
  toConnectionRow,
} from "./lib/rows.js";
export type { ConnectionModelFlags, ConnectionRow, ModelChoice, ModelControlKind, ModelCredit } from "./lib/rows.js";
export {
  SIGN_IN_POLL_INTERVAL_MS,
  SIGN_IN_POLL_TIMEOUT_MS,
  beginProviderSignIn,
  pollProviderLogin,
} from "./lib/signIn.js";
export type { PollClock, PollOutcome, SignInFinish, SignInStartFailure, SignInStartResult } from "./lib/signIn.js";
export { finishNotice, startFailureNotice } from "./lib/report.js";
export type { Notice } from "./lib/report.js";
export { formatResetTime, formatTokenCount, freeTokenMeterText } from "./lib/quota.js";
export { registerCloudAssistantSettings } from "./register.js";
