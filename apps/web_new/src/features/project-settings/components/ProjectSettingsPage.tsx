// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-tab page chrome (PSET-001; parity/specs/project-settings-shell.md).
// The header row names the area, carries the project emblem and titles
// the open tab; the body is a width-capped scrolling column. Tabs whose
// owning slice brings its own screen (labels, automations) skip this
// header and mount their screen directly.

import * as React from "react";

import { getAppBaseUrl } from "../../../core/api/client.js";
import { ProjectLogo } from "../../projects/index.js";
import { useProjectSettings } from "./ProjectSettingsLayout.js";

export interface ProjectSettingsPageProps {
  title: string;
  children: React.ReactNode;
}

export function ProjectSettingsPage({ title, children }: ProjectSettingsPageProps): React.ReactElement {
  const { project } = useProjectSettings();
  return (
    <div className="flex min-h-0 flex-col" data-testid="project-settings-page">
      <header className="flex shrink-0 items-center gap-(--space-3) border-b border-(--border) px-(--space-6) py-(--space-4)">
        {project === null ? null : (
          <ProjectLogo logoProps={project.logoProps} identifier={project.identifier} apiBaseUrl={getAppBaseUrl()} />
        )}
        <div className="flex min-w-0 flex-col">
          <p className="text-caption truncate text-(--text-muted)">Project settings</p>
          <h1 className="text-h3 truncate text-(--text)">{title}</h1>
        </div>
      </header>
      <div className="mx-auto w-full max-w-3xl px-(--space-6) py-(--space-8)">{children}</div>
    </div>
  );
}
