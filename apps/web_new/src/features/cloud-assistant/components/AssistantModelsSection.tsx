// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-surface Models section of the cloud assistant tab (NEWPIDASH-394,
// design sections 7.1 and 7.4). Three pickers — web assistant, desktop
// assistant, and the Pi Dash Agent — over the merged catalog-plus-BYOK
// list; the agent sees catalog models only (design D-12). Choices and
// the credit-blocked guard come from the connections list and the quota
// read the tab already holds, so this section takes them as props.

import * as React from "react";

import type { AssistantConnectionList, FreeQuota } from "@pidash/api-client";

import {
  MODELS_SECTION_HASH,
  SurfaceModelPickers,
  applyCreditBadges,
  buildMergedModelChoices,
  byokKeysFromList,
  catalogFromList,
  surfacePickerLabel,
} from "../../assistant/index.js";
import { connectionModelFlags } from "../lib/rows.js";
import { MODELS_SECTION_HEADING, MODELS_SECTION_INTRO } from "../lib/copy.js";

export interface AssistantModelsSectionProps {
  /** Workspace scoping the selection read the pickers refresh. */
  workspaceSlug: string;
  /** The connections list backing the merged choices and the credit guard. */
  list: AssistantConnectionList;
  /** The free quota backing the credit badges; null when unavailable. */
  quota: FreeQuota | null;
}

export function AssistantModelsSection({
  workspaceSlug,
  list,
  quota,
}: AssistantModelsSectionProps): React.ReactElement | null {
  const flags = React.useMemo(() => connectionModelFlags(list, quota), [list, quota]);
  const specs = React.useMemo(() => {
    const merged = applyCreditBadges(buildMergedModelChoices(catalogFromList(list), byokKeysFromList(list)), flags);
    if (merged.length === 0) return [];
    const agentChoices = merged.filter((choice) => choice.origin === "catalog");
    return [
      { surface: "web_assistant" as const, label: surfacePickerLabel("web_assistant"), choices: merged },
      { surface: "desktop_assistant" as const, label: surfacePickerLabel("desktop_assistant"), choices: merged },
      { surface: "agent" as const, label: surfacePickerLabel("agent"), choices: agentChoices },
    ];
  }, [list, flags]);
  if (specs.length === 0) return null;

  const headingId = "assistant-models-heading";
  return (
    <section
      aria-labelledby={headingId}
      data-testid="assistant-models-section"
      id={MODELS_SECTION_HASH}
      className="flex flex-col gap-(--space-3)"
    >
      <div className="flex flex-col gap-(--space-1)">
        <h2 id={headingId} className="text-emphasis font-medium text-(--text)">
          {MODELS_SECTION_HEADING}
        </h2>
        <p className="text-body text-(--text-muted)">{MODELS_SECTION_INTRO}</p>
      </div>
      <SurfaceModelPickers workspaceSlug={workspaceSlug} surfaces={specs} flags={flags} />
    </section>
  );
}
