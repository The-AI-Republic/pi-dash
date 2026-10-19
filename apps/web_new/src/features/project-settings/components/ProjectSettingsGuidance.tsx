// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bare settings path with nothing to forward to (PSET-002, from
// parity/specs/project-settings-shell.md). A member of no project reads
// a short explanation and a way to create one; the creation form is the
// shared project dialog owned by its lifecycle slice.

import { Button } from "@pidash/kit";
import { ArrowUpRight, FolderPlus } from "lucide-react";
import * as React from "react";

import { DOCS_URL } from "../../../shared/shell/externalLinks.js";

export const NO_PROJECTS_HEADING = "No projects yet";
export const NO_PROJECTS_BODY =
  "Projects hold the work items, cycles and agents for one effort. Create the first one to open its settings.";
export const START_FIRST_PROJECT_LABEL = "Start your first project";
export const PROJECTS_GUIDE_LABEL = "Read about projects";

export interface ProjectSettingsGuidanceProps {
  onCreate: () => void;
}

export function ProjectSettingsGuidance({ onCreate }: ProjectSettingsGuidanceProps): React.ReactElement {
  return (
    <section
      aria-label={NO_PROJECTS_HEADING}
      data-testid="project-settings-guidance"
      className="flex h-full min-h-64 flex-col items-center justify-center gap-(--space-3) p-(--space-12) text-center"
    >
      <span className="flex size-12 items-center justify-center rounded-(--radius-dialog) bg-(--subtle) text-(--text-muted)">
        <FolderPlus size={24} aria-hidden="true" />
      </span>
      <h1 className="text-h3 text-(--text)">{NO_PROJECTS_HEADING}</h1>
      <p className="text-body max-w-md text-(--text-muted)">{NO_PROJECTS_BODY}</p>
      <div className="flex flex-wrap items-center justify-center gap-(--space-2)">
        <a
          href={DOCS_URL}
          target="_blank"
          rel="noopener noreferrer"
          className="text-body inline-flex items-center gap-(--space-1) rounded-(--radius-control) px-(--space-3) py-(--space-2) text-(--text-muted) hover:text-(--text)"
        >
          {PROJECTS_GUIDE_LABEL}
          <ArrowUpRight size={14} aria-hidden="true" />
        </a>
        <Button variant="primary" onClick={onCreate}>
          {START_FIRST_PROJECT_LABEL}
        </Button>
      </div>
    </section>
  );
}
