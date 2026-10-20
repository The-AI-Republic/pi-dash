// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project feature switches (parity/specs/project-settings-part-f.md,
// PSET-043–048). Pure: each settings tab owns one boolean on the project
// row, and the product areas that hide or return when it flips are
// listed here so the settings stores and the project navigation read the
// same map. Nothing here reads React, the router or the network.

/** The five switches a project admin can flip. */
export const PROJECT_FEATURE_KEYS = ["views", "cycles", "intake", "modules", "pages"] as const;
export type ProjectFeatureKey = (typeof PROJECT_FEATURE_KEYS)[number];

/** The boolean each switch writes. */
export type ProjectFeatureFlag = "issue_views_view" | "cycle_view" | "inbox_view" | "module_view" | "page_view";

export interface ProjectFeatureDef {
  key: ProjectFeatureKey;
  /** Settings tab heading, matching the sidebar label. */
  label: string;
  /** The single project field the switch writes. */
  flag: ProjectFeatureFlag;
  /** Switch title inside the settings body. */
  switchLabel: string;
  /** One-line explainer beside the switch. */
  explainer: string;
  /** Settings route suffix. */
  suffix: string;
  /** Product navigation segment that disappears while the flag is off. */
  areaSegment: string;
}

export const PROJECT_FEATURES: readonly ProjectFeatureDef[] = [
  {
    key: "views",
    label: "Views",
    flag: "issue_views_view",
    switchLabel: "Enable views",
    explainer: "Let people save a filter, sort and layout combination and return to it later.",
    suffix: "/features/views",
    areaSegment: "views",
  },
  {
    key: "cycles",
    label: "Cycles",
    flag: "cycle_view",
    switchLabel: "Enable cycles",
    explainer: "Group work into planning periods whose length can change from one period to the next.",
    suffix: "/features/cycles",
    areaSegment: "cycles",
  },
  {
    key: "intake",
    label: "Intake",
    flag: "inbox_view",
    switchLabel: "Enable intake",
    explainer: "Collect and discuss incoming requests before deciding where they belong.",
    suffix: "/features/intake",
    areaSegment: "intake",
  },
  {
    key: "modules",
    label: "Modules",
    flag: "module_view",
    switchLabel: "Enable modules",
    explainer: "Carve the project into smaller bodies of work with their own owners.",
    suffix: "/features/modules",
    areaSegment: "modules",
  },
  {
    key: "pages",
    label: "Pages",
    flag: "page_view",
    switchLabel: "Enable pages",
    explainer: "Write documents next to the work so notes and specs stay in one place.",
    suffix: "/features/pages",
    areaSegment: "pages",
  },
];

/** The switch definition for a tab key, or undefined outside the five. */
export function findProjectFeature(key: string): ProjectFeatureDef | undefined {
  return PROJECT_FEATURES.find((feature) => feature.key === key);
}

/** Index of the same five definitions by key; a registry test proves it whole. */
const PROJECT_FEATURE_INDEX = PROJECT_FEATURES.reduce<Record<ProjectFeatureKey, ProjectFeatureDef>>(
  (index, feature) => ({ ...index, [feature.key]: feature }),
  {} as Record<ProjectFeatureKey, ProjectFeatureDef>
);

/** The switch definition a route leaf mounts. Every key resolves. */
export function requireProjectFeature(key: ProjectFeatureKey): ProjectFeatureDef {
  return PROJECT_FEATURE_INDEX[key];
}

/** Whether the project row carries the flag switched on. */
export function isFeatureEnabled(
  project: Partial<Record<ProjectFeatureFlag, boolean>> | undefined,
  feature: ProjectFeatureDef
): boolean {
  return project?.[feature.flag] === true;
}

/**
 * The next project value for a switch: the inverse of what the row shows.
 * Undefined flags read as off, so the first press always turns one on.
 */
export function nextFeatureValue(
  project: Partial<Record<ProjectFeatureFlag, boolean>> | undefined,
  feature: ProjectFeatureDef
): boolean {
  return !isFeatureEnabled(project, feature);
}
