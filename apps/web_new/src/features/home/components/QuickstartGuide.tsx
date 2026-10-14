// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Quickstart checklist (SHELL-008, SHELL-009, SHELL-010; written from
// parity/specs/shell.md §2 and the quickstart oracle). Four cards onboard
// fresh workspaces above the widget stack: each action is role-gated with
// no affordance where forbidden, and each card tracks its own completion.
// The route passes the project list in (features never import each other,
// so the route composes the cross-feature read). Dismissal and settings
// and profile visits persist per workspace in local storage.

import { Badge, Button } from "@pidash/kit";
import type { ProjectList } from "@pidash/api-client";
import { Check, CircleUserRound, FolderPlus, SlidersHorizontal, UserPlus, X } from "lucide-react";
import * as React from "react";

import { usePermissions, useWorkspace } from "../../../core/session/index.js";
import { CrossAppLink } from "../../../shared/coexistence/index.js";
import {
  QUICKSTART_ACTIONS,
  QUICKSTART_CARD_IDS,
  QUICKSTART_DESCRIPTIONS,
  QUICKSTART_TITLES,
  canShowQuickstartAction,
  countJoinedProjects,
  isQuickstartCardDone,
  isQuickstartPanelVisible,
  quickstartLinkHref,
  quickstartVisitFlag,
  readQuickstartFlags,
  resolveQuickstartStorage,
  writeQuickstartFlags,
  type QuickstartCardId,
  type QuickstartFlags,
  type QuickstartLinkCardId,
  type QuickstartVisitFlag,
} from "../lib/quickstart.js";

const CARD_ICONS = {
  "create-project": FolderPlus,
  "invite-team": UserPlus,
  "configure-workspace": SlidersHorizontal,
  "personalize-account": CircleUserRound,
} as const;

export interface QuickstartGuideProps {
  workspaceSlug: string;
  /** Workspace projects; the create card completes off the joined rows. */
  projects: ProjectList | undefined;
  /**
   * Opens the create-project dialog. Owned by the projects area
   * (NEWFRONT-284); until it lands, the create action links to the
   * projects list instead.
   */
  onCreateProject?: (() => void) | undefined;
}

export function QuickstartGuide({
  workspaceSlug,
  projects,
  onCreateProject,
}: QuickstartGuideProps): React.ReactElement | null {
  const { workspace } = useWorkspace(workspaceSlug);
  const permissions = usePermissions(workspaceSlug);
  const [flags, setFlags] = React.useState<QuickstartFlags>(() =>
    readQuickstartFlags(resolveQuickstartStorage(), workspaceSlug)
  );
  if (projects === undefined || workspace === undefined) return null;
  const joinedProjectCount = countJoinedProjects(projects);
  const input = { joinedProjectCount, totalMembers: workspace.total_members, flags };
  if (!isQuickstartPanelVisible(input)) return null;

  const storeFlags = (next: QuickstartFlags): void => {
    writeQuickstartFlags(resolveQuickstartStorage(), workspaceSlug, next);
    setFlags(next);
  };
  const dismiss = (): void => {
    storeFlags({ ...flags, hide: true });
  };
  const recordVisit = (visit: QuickstartVisitFlag): void => {
    storeFlags({ ...flags, [visit]: true });
  };

  return (
    <section aria-label="Quickstart checklist" id="home-quickstart" className="flex flex-col gap-(--space-4)">
      <div className="flex items-center justify-between gap-(--space-4)">
        <h2 className="text-h3 text-(--text)">Quickstart checklist</h2>
        <Button type="button" variant="ghost" size="small" onClick={dismiss}>
          <X size={14} aria-hidden="true" />
          Dismiss
        </Button>
      </div>
      <div className="grid grid-cols-1 gap-(--space-4) md:grid-cols-2">
        {QUICKSTART_CARD_IDS.map((cardId) => (
          <QuickstartCard
            key={cardId}
            cardId={cardId}
            workspaceSlug={workspaceSlug}
            done={isQuickstartCardDone(cardId, input)}
            showAction={canShowQuickstartAction(cardId, permissions)}
            onCreateProject={onCreateProject}
            onVisit={recordVisit}
          />
        ))}
      </div>
    </section>
  );
}

interface QuickstartCardProps {
  cardId: QuickstartCardId;
  workspaceSlug: string;
  done: boolean;
  showAction: boolean;
  onCreateProject: (() => void) | undefined;
  onVisit: (visit: QuickstartVisitFlag) => void;
}

function QuickstartCard({
  cardId,
  workspaceSlug,
  done,
  showAction,
  onCreateProject,
  onVisit,
}: QuickstartCardProps): React.ReactElement {
  const Icon = CARD_ICONS[cardId];
  return (
    <div
      data-card={cardId}
      data-done={done ? "true" : "false"}
      className="flex flex-col gap-(--space-2) rounded-(--radius-dialog) border border-(--border) bg-(--surface) p-(--space-4)"
    >
      <span className="grid size-9 place-items-center rounded-full bg-(--subtle) text-(--text-muted)">
        <Icon size={18} aria-hidden="true" />
      </span>
      <h3 className="text-emphasis text-(--text)">{QUICKSTART_TITLES[cardId]}</h3>
      <p className="text-body text-(--text-muted)">{QUICKSTART_DESCRIPTIONS[cardId]}</p>
      {done ? (
        <Badge tone="success" dot className="w-fit">
          <Check size={12} aria-hidden="true" />
          Done
        </Badge>
      ) : showAction ? (
        <QuickstartAction
          cardId={cardId}
          workspaceSlug={workspaceSlug}
          onCreateProject={onCreateProject}
          onVisit={onVisit}
        />
      ) : null}
    </div>
  );
}

interface QuickstartActionProps {
  cardId: QuickstartCardId;
  workspaceSlug: string;
  onCreateProject: (() => void) | undefined;
  onVisit: (visit: QuickstartVisitFlag) => void;
}

function QuickstartAction({
  cardId,
  workspaceSlug,
  onCreateProject,
  onVisit,
}: QuickstartActionProps): React.ReactElement {
  const label = QUICKSTART_ACTIONS[cardId];
  if (cardId === "create-project") {
    if (onCreateProject !== undefined) {
      return (
        <Button type="button" variant="ghost" size="small" onClick={onCreateProject} className="self-start">
          {label}
        </Button>
      );
    }
    return (
      <CrossAppLink
        to={`/${workspaceSlug}/projects`}
        className="text-body self-start font-medium text-(--accent) hover:underline"
      >
        {label}
      </CrossAppLink>
    );
  }
  const linkCardId: QuickstartLinkCardId = cardId;
  const visit = quickstartVisitFlag(linkCardId);
  return (
    <CrossAppLink
      to={quickstartLinkHref(linkCardId, workspaceSlug)}
      {...(visit === null ? {} : { onClick: () => onVisit(visit) })}
      className="text-body self-start font-medium text-(--accent) hover:underline"
    >
      {label}
    </CrossAppLink>
  );
}
