// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-surface Models section of the OSS assistant settings page
// (NEWPIDASH-394, design sections 7.1 and 7.4). Two pickers — web
// assistant and desktop assistant — over the single key's model list;
// the OSS edition has no catalog lane, so the merged list is the key
// alone and the PUT sends a null connection. The agent surface is
// cloud-only and stays out.

import * as React from "react";

import type { LlmConfig } from "@pidash/api-client";

import {
  MODELS_SECTION_HASH,
  SurfaceModelPickers,
  buildMergedModelChoices,
  byokKeyFromConfig,
  surfacePickerLabel,
} from "../../assistant/index.js";
import { SectionFrame } from "./AssistantSections.js";
import { MODELS_SECTION_HEADING, MODELS_SECTION_INTRO } from "../lib/copy.js";

export interface AssistantModelsSectionProps {
  /** Workspace scoping the selection read the pickers refresh. */
  workspaceSlug: string;
  /** The signed-in member's key config backing the BYOK half. */
  config: LlmConfig;
}

export function AssistantModelsSection({
  workspaceSlug,
  config,
}: AssistantModelsSectionProps): React.ReactElement | null {
  const specs = React.useMemo(() => {
    const key = byokKeyFromConfig(config);
    const choices = buildMergedModelChoices({ models: [], labels: {}, descriptions: {} }, [key]);
    if (choices.length === 0) return [];
    return [
      { surface: "web_assistant" as const, label: surfacePickerLabel("web_assistant"), choices },
      { surface: "desktop_assistant" as const, label: surfacePickerLabel("desktop_assistant"), choices },
    ];
  }, [config]);
  if (specs.length === 0) return null;

  return (
    <SectionFrame
      heading={MODELS_SECTION_HEADING}
      description={MODELS_SECTION_INTRO}
      testId="assistant-models-section"
      id={MODELS_SECTION_HASH}
    >
      <SurfaceModelPickers workspaceSlug={workspaceSlug} surfaces={specs} flags={null} />
    </SectionFrame>
  );
}
