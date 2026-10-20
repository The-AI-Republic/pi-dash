// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the project-features feature. Other features and routes
// import only from here.

export {
  FEATURE_NOT_AUTHORIZED_BODY,
  FEATURE_NOT_AUTHORIZED_TITLE,
  FEATURE_SETTINGS_CONTEXT,
  FEATURE_UPDATE_FAILED_MESSAGE,
  FEATURE_UPDATE_FAILED_TITLE,
  FEATURE_UPDATE_PENDING_MESSAGE,
  FEATURE_UPDATE_PENDING_TITLE,
  FEATURE_UPDATE_SUCCESS_MESSAGE,
  FEATURE_UPDATE_SUCCESS_TITLE,
  useUpdateProjectFeature,
} from "./api/featureQueries.js";
export type { FeaturePatch, UpdateFeatureInput } from "./api/featureQueries.js";
export {
  findProjectFeature,
  isFeatureEnabled,
  nextFeatureValue,
  PROJECT_FEATURES,
  PROJECT_FEATURE_KEYS,
  requireProjectFeature,
} from "./lib/registry.js";
export type { ProjectFeatureDef, ProjectFeatureFlag, ProjectFeatureKey } from "./lib/registry.js";
export { ProjectFeatureSettings } from "./components/ProjectFeatureSettings.js";
export type { ProjectFeatureSettingsProps } from "./components/ProjectFeatureSettings.js";
