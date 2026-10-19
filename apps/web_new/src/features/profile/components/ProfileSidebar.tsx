// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Identity side panel (parity/specs/profile.md §4, PROF-008). Target
// behavior: no cover art anywhere; the avatar photo with an uppercase
// initial fallback; the full name with the display name beside it; the
// join date; a clock reading the member's own timezone that ticks every
// minute; and one expandable row per project carrying its completion tint
// and created/assigned/due/completed counts. On your own profile an edit
// pencil links to the settings page general tab. The panel collapses on
// demand and, on narrow screens, starts collapsed and floats over the
// content.

import { Avatar, IconButton } from "@pidash/kit";
import { ChevronDown, PanelRight, Pencil } from "lucide-react";
import * as React from "react";

import type { UserProfileProject, UserProfileSegregation } from "@pidash/api-client";
import { CrossAppLink } from "../../../shared/coexistence/CrossAppLink.js";
import { resolveFileUrl } from "../../../shared/shell/fileUrls.js";
import {
  formatJoinDate,
  profileClock,
  profileDisplayName,
  profileFullName,
  profileInitial,
  projectCompletion,
  projectCompletionRatio,
} from "../lib/identity.js";

export const PROFILE_SETTINGS_GENERAL_HREF = "/settings/profile/general";

/** How often the clock re-reads the wall time. */
export const PROFILE_CLOCK_TICK_MS = 60_000;

/** One project row's name, resolved from the workspace project list. */
export interface ProfileProjectName {
  id: string;
  name: string;
}

/** Ticking wall-clock reading for a timezone. */
export function useProfileClock(timeZone: string): string {
  const [reading, setReading] = React.useState(() => profileClock(timeZone));
  React.useEffect(() => {
    setReading(profileClock(timeZone));
    const timer = globalThis.setInterval(() => {
      setReading(profileClock(timeZone));
    }, PROFILE_CLOCK_TICK_MS);
    return () => {
      globalThis.clearInterval(timer);
    };
  }, [timeZone]);
  return reading;
}

function ProjectRow({ project, name }: { project: UserProfileProject; name: string }): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const tint = projectCompletionRatio(project.assigned_issues, project.completed_issues);
  return (
    <li data-profile-project-row={project.id} className="border-b border-(--border) last:border-b-0">
      <button
        type="button"
        aria-expanded={open}
        aria-label={`${name}, ${projectCompletion(project.assigned_issues, project.completed_issues)} complete`}
        onClick={() => {
          setOpen((current) => !current);
        }}
        className="flex w-full items-center justify-between gap-(--space-2) px-(--space-3) py-(--space-2) text-left hover:bg-(--subtle)"
      >
        <span className="flex min-w-0 items-center gap-(--space-2)">
          <span
            aria-hidden="true"
            data-profile-project-tint={project.id}
            className="size-2 shrink-0 rounded-(--radius-full)"
            style={{ backgroundColor: `color-mix(in srgb, var(--accent) ${tint}%, var(--border))` }}
          />
          <span className="text-body min-w-0 truncate text-(--text)">{name}</span>
        </span>
        <span className="flex items-center gap-(--space-1)">
          <span className="text-caption text-(--text-muted)" data-profile-project-completion={project.id}>
            {projectCompletion(project.assigned_issues, project.completed_issues)}
          </span>
          <ChevronDown
            size={14}
            strokeWidth={1.5}
            aria-hidden="true"
            className={`text-(--text-muted) transition-transform ${open ? "rotate-180" : ""}`}
          />
        </span>
      </button>
      {open ? (
        <dl
          data-profile-project-counts={project.id}
          className="grid grid-cols-2 gap-(--space-1) px-(--space-3) pb-(--space-3)"
        >
          <Count label="Created" value={project.created_issues} />
          <Count label="Assigned" value={project.assigned_issues} />
          <Count label="Due" value={project.pending_issues} />
          <Count label="Completed" value={project.completed_issues} />
        </dl>
      ) : null}
    </li>
  );
}

function Count({ label, value }: { label: string; value: number }): React.ReactElement {
  return (
    <div className="flex items-baseline justify-between gap-(--space-2)">
      <dt className="text-caption text-(--text-muted)">{label}</dt>
      <dd className="text-caption text-(--text)">{String(value)}</dd>
    </div>
  );
}

export interface ProfileSidebarProps {
  segregation: UserProfileSegregation;
  /** Workspace slug, for the settings link and nothing else. */
  workspaceSlug: string;
  /** Viewed member is the signed-in member. */
  isOwn: boolean;
  /** Project names by id; projects missing a workspace row fall back to their id. */
  projects: readonly ProfileProjectName[];
  /** API base for relative avatar paths. */
  apiBaseUrl: string;
}

/** The identity content, shared by the docked panel and the floating sheet. */
function ProfileIdentity({
  segregation,
  isOwn,
  projects,
  apiBaseUrl,
  onCollapse,
}: ProfileSidebarProps & { onCollapse: () => void }): React.ReactElement {
  const identity = segregation.user_data;
  const clock = useProfileClock(identity.user_timezone);
  const avatarSrc = resolveFileUrl(identity.avatar_url, apiBaseUrl);
  const names = new Map(projects.map((project) => [project.id, project.name]));
  return (
    <aside
      aria-label="Profile details"
      data-testid="profile-sidebar"
      className="flex h-full w-72 shrink-0 flex-col overflow-y-auto border-r border-(--border) bg-(--surface)"
    >
      <div className="flex items-center justify-between px-(--space-3) pt-(--space-3)">
        <span className="text-caption font-medium tracking-wide text-(--text-muted) uppercase">Profile</span>
        <IconButton label="Hide profile details" size="small" onClick={onCollapse}>
          <PanelRight size={16} strokeWidth={1.5} aria-hidden="true" />
        </IconButton>
      </div>
      <div className="flex flex-col items-center gap-(--space-2) px-(--space-3) py-(--space-4)">
        {avatarSrc === null ? (
          <span
            role="img"
            aria-label={profileFullName(identity)}
            data-profile-avatar="initial"
            className="text-h3 grid size-16 place-items-center rounded-(--radius-full) bg-(--subtle) text-(--text-muted)"
          >
            {profileInitial(identity)}
          </span>
        ) : (
          <Avatar name={profileFullName(identity)} src={avatarSrc} size="large" className="text-h3 size-16" />
        )}
        <div className="flex flex-col items-center gap-(--space-1) text-center">
          <p data-profile-name="full" className="text-body font-semibold text-(--text)">
            {profileFullName(identity)}
          </p>
          {profileDisplayName(identity.display_name) === null ? null : (
            <p data-profile-name="display" className="text-caption text-(--text-muted)">
              {profileDisplayName(identity.display_name)}
            </p>
          )}
        </div>
        {isOwn ? (
          <CrossAppLink
            to={PROFILE_SETTINGS_GENERAL_HREF}
            aria-label="Edit profile"
            data-testid="profile-edit-pencil"
            className="text-body flex items-center gap-(--space-1) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-(--text-muted) hover:bg-(--subtle) hover:text-(--text)"
          >
            <Pencil size={14} strokeWidth={1.5} aria-hidden="true" />
            <span>Edit</span>
          </CrossAppLink>
        ) : null}
      </div>
      <dl className="flex flex-col gap-(--space-2) border-t border-(--border) px-(--space-3) py-(--space-3)">
        <div className="flex items-baseline justify-between gap-(--space-2)">
          <dt className="text-caption text-(--text-muted)">Joined on</dt>
          <dd data-profile-join-date className="text-caption text-(--text)">
            {formatJoinDate(identity.date_joined)}
          </dd>
        </div>
        <div className="flex items-baseline justify-between gap-(--space-2)">
          <dt className="text-caption text-(--text-muted)">Local time</dt>
          <dd data-profile-clock className="text-caption text-(--text)">
            {clock}
          </dd>
        </div>
      </dl>
      <div className="border-t border-(--border)">
        <p className="text-caption px-(--space-3) py-(--space-2) font-medium text-(--text-muted)">Projects</p>
        <ul aria-label="Profile projects" className="flex flex-col">
          {segregation.project_data.map((project) => (
            <ProjectRow key={project.id} project={project} name={names.get(project.id) ?? project.id} />
          ))}
        </ul>
      </div>
    </aside>
  );
}

export interface ProfileSidebarFrameProps extends ProfileSidebarProps {
  /** Viewport is narrow: the panel floats over content. */
  narrow: boolean;
}

/**
 * Places the identity panel. On narrow screens it starts collapsed and
 * opens as a floating sheet over the content with a dismissable backdrop;
 * wide screens dock it beside the content. The show control always
 * renders (beside the header when open, in place of the panel when
 * closed) so the panel can be toggled in every viewport.
 */
export function ProfileSidebarFrame(props: ProfileSidebarFrameProps): React.ReactElement {
  const { narrow } = props;
  const [open, setOpen] = React.useState(!narrow);
  React.useEffect(() => {
    setOpen(!narrow);
  }, [narrow]);
  const toggle = (next: boolean) => {
    setOpen(next);
  };
  return (
    <>
      {open && narrow ? (
        <button
          type="button"
          aria-label="Close profile details"
          data-testid="profile-sidebar-backdrop"
          onClick={() => {
            toggle(false);
          }}
          className="fixed inset-0 z-30 bg-black/35"
        />
      ) : null}
      {open ? (
        <div className={narrow ? "shadow-xl fixed inset-y-0 left-0 z-40" : "relative flex h-full shrink-0"}>
          <ProfileIdentity
            {...props}
            onCollapse={() => {
              toggle(false);
            }}
          />
        </div>
      ) : (
        <IconButton
          label="Show profile details"
          size="small"
          data-testid="profile-sidebar-toggle"
          onClick={() => {
            toggle(true);
          }}
          className="m-(--space-2) shrink-0"
        >
          <PanelRight size={16} strokeWidth={1.5} aria-hidden="true" />
        </IconButton>
      )}
    </>
  );
}
