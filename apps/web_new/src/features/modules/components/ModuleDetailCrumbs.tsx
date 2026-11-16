// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module detail trail (parity/specs/modules.md §8.1, MOD-029).
// Project → Modules → the module itself, where the last crumb is the
// module switcher control (a searchable ModulePicker over every
// non-archived project module), never an inert label. Narrow screens
// collapse the trail to a back control returning to the modules list.

import { Link, useNavigate } from "@tanstack/react-router";
import { ChevronDown, ChevronLeft, ChevronRight } from "lucide-react";
import * as React from "react";

import { ModulePicker } from "../../../shared/pickers/index.js";
import { MODULES_CRUMB, modulesListHref } from "../lib/moduleList.js";
import { projectIssuesHref, switchModuleTarget } from "../lib/moduleDetail.js";

export interface ModuleDetailCrumbsProps {
  workspaceSlug: string;
  projectId: string;
  projectName: string;
  moduleId: string;
  moduleName: string;
}

export function ModuleDetailCrumbs({
  workspaceSlug,
  projectId,
  projectName,
  moduleId,
  moduleName,
}: ModuleDetailCrumbsProps): React.ReactElement {
  const navigate = useNavigate();
  const listHref = modulesListHref(workspaceSlug, projectId);
  return (
    <>
      <nav aria-label="Breadcrumb" data-testid="module-detail-crumbs" className="hidden shrink-0 md:block">
        <ol className="flex min-w-0 items-center gap-(--space-1)">
          <li className="flex min-w-0 items-center gap-(--space-1)">
            <Link
              to={projectIssuesHref(workspaceSlug, projectId)}
              data-testid="module-crumb-project"
              className="text-body flex min-w-0 items-center gap-(--space-1) rounded-(--radius-control) text-(--text-muted) hover:text-(--text)"
            >
              <span className="max-w-40 truncate">{projectName}</span>
            </Link>
          </li>
          <li className="flex min-w-0 items-center gap-(--space-1)" aria-hidden="true">
            <ChevronRight size={14} strokeWidth={1.5} className="shrink-0 text-(--text-muted)" />
          </li>
          <li className="flex min-w-0 items-center gap-(--space-1)">
            <Link
              to={listHref}
              data-testid="module-crumb-modules"
              className="text-body flex min-w-0 items-center gap-(--space-1) rounded-(--radius-control) text-(--text-muted) hover:text-(--text)"
            >
              {MODULES_CRUMB}
            </Link>
          </li>
          <li className="flex min-w-0 items-center gap-(--space-1)" aria-hidden="true">
            <ChevronRight size={14} strokeWidth={1.5} className="shrink-0 text-(--text-muted)" />
          </li>
          <li className="flex min-w-0 items-center gap-(--space-1)">
            <ModulePicker
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              value={moduleId}
              onChange={(id) => {
                const target = switchModuleTarget(workspaceSlug, projectId, moduleId, id);
                if (target !== null) {
                  void navigate({ to: target });
                }
              }}
              allowClear={false}
              trigger={
                <button
                  type="button"
                  data-testid="module-switcher-trigger"
                  aria-label={`Switch module, current ${moduleName}`}
                  className="text-body flex min-w-0 items-center gap-(--space-1) rounded-(--radius-control) text-(--text)"
                >
                  <span className="max-w-40 truncate">{moduleName}</span>
                  <ChevronDown
                    size={14}
                    strokeWidth={1.5}
                    aria-hidden="true"
                    className="shrink-0 text-(--text-muted)"
                  />
                </button>
              }
            />
          </li>
        </ol>
      </nav>
      <div className="shrink-0 md:hidden">
        <Link
          to={listHref}
          data-testid="module-mobile-back"
          aria-label="Back to modules"
          className="text-body inline-flex items-center gap-(--space-1) rounded-(--radius-control) text-(--text-muted) hover:text-(--text)"
        >
          <ChevronLeft size={16} strokeWidth={1.5} aria-hidden="true" />
          {MODULES_CRUMB}
        </Link>
      </div>
    </>
  );
}
