// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Repository tab internals for the card's own suites.

export {
  cadenceLine,
  providerLabel,
  REPOSITORY_BOUND_PREFIX,
  REPOSITORY_CADENCE,
  REPOSITORY_CARD_HEADING,
  REPOSITORY_CONTRACT,
  REPOSITORY_GUIDANCE_BODY,
  REPOSITORY_LAST_ERROR_PREFIX,
  REPOSITORY_SYNC_LABEL,
  REPOSITORY_UNBIND_LABEL,
  RepositorySyncCard,
} from "./RepositorySyncCard.js";
export type { RepositorySyncCardProps } from "./RepositorySyncCard.js";
export {
  repositoryKeys,
  repositoryQueryOptions,
  SYNC_FAILED_BODY,
  SYNC_FAILED_TITLE,
  SYNC_UPDATED_BODY,
  SYNC_UPDATED_TITLE,
  UNBOUND_BODY,
  UNBOUND_TITLE,
  useRepositoryStatus,
  useToggleRepositorySync,
  useUnbindRepository,
} from "./api.js";
