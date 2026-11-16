// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Projects-list filter panel (SHELL-029). Ownership, access, lead,
// members and creation date narrow the fetched collection; the value
// search matches options case-insensitively and the trigger carries a
// badge while panel values apply. Written from parity/specs/shell.md
// §3; the area gate drives it by visible labels.

import { Avatar, Button, Popover, Skeleton } from "@pidash/kit";
import { ChevronDown, Globe, ListFilter, Lock, Search, X } from "lucide-react";
import * as React from "react";
import type { WorkspaceMember } from "@pidash/api-client";

import {
  ACCESS_OPTIONS,
  countActiveListFilters,
  CREATED_PRESETS,
  isCustomCreatedValue,
  selectMemberOptions,
  type ListFilters,
  type PersonFilterDimension,
} from "../lib/list-display.js";
import { ProjectsCustomDateDialog } from "./ProjectsCustomDateDialog.js";

export interface ProjectsFilterPanelProps {
  filters: ListFilters;
  /** Membership options; undefined while resolving. */
  members: WorkspaceMember[] | undefined;
  selfId: string | null;
  onToggleMyProjects: () => void;
  onToggleAccess: (value: number) => void;
  onTogglePerson: (dimension: PersonFilterDimension, id: string) => void;
  onSelectCreated: (values: string[]) => void;
}

type SectionKey = "access" | "lead" | "members" | "created";

const VISIBLE_PEOPLE = 5;

function SectionHeader({
  title,
  count,
  collapsed,
  onToggle,
}: {
  title: string;
  count: number;
  collapsed: boolean;
  onToggle: () => void;
}): React.ReactElement {
  return (
    <button
      type="button"
      aria-expanded={!collapsed}
      onClick={onToggle}
      className="text-meta flex w-full cursor-pointer items-center justify-between py-(--space-2) font-medium text-(--text-muted)"
    >
      <span>
        {title}
        {count > 0 ? ` (${count})` : ""}
      </span>
      <ChevronDown
        size={14}
        strokeWidth={2}
        aria-hidden="true"
        className={collapsed ? "-rotate-90 transition-transform" : "transition-transform"}
      />
    </button>
  );
}

function PersonSection({
  title,
  dimension,
  applied,
  memberships,
  selfId,
  query,
  collapsed,
  onToggleSection,
  onTogglePerson,
}: {
  title: string;
  dimension: PersonFilterDimension;
  applied: string[];
  memberships: WorkspaceMember[] | undefined;
  selfId: string | null;
  query: string;
  collapsed: boolean;
  onToggleSection: () => void;
  onTogglePerson: (dimension: PersonFilterDimension, id: string) => void;
}): React.ReactElement {
  const [expanded, setExpanded] = React.useState(false);
  const options = memberships === undefined ? null : selectMemberOptions(memberships, { applied, selfId, query });
  const visible = options === null || expanded ? options : options.slice(0, VISIBLE_PEOPLE);

  return (
    <section aria-label={title}>
      <SectionHeader title={title} count={applied.length} collapsed={collapsed} onToggle={onToggleSection} />
      {collapsed ? null : (
        <div className="flex flex-col gap-(--space-1)">
          {visible === null ? (
            <div role="status" aria-label="Loading people" className="flex flex-col gap-(--space-2)">
              <Skeleton height="20px" />
              <Skeleton height="20px" width="80%" />
            </div>
          ) : visible.length === 0 ? (
            <p className="text-body text-(--text-muted) italic">No matches found</p>
          ) : (
            visible.map((option) => (
              <label
                key={option.id}
                className="text-body flex cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) hover:bg-(--subtle)"
              >
                <input type="checkbox" checked={option.applied} onChange={() => onTogglePerson(dimension, option.id)} />
                {/* Decorative here: the label text already names the option,
                    so the avatar's img role must not leak into the name. */}
                <span aria-hidden="true">
                  <Avatar name={option.displayName} src={option.avatarUrl} size="small" />
                </span>
                {option.self ? "You" : option.displayName}
              </label>
            ))
          )}
          {options !== null && options.length > VISIBLE_PEOPLE ? (
            <button
              type="button"
              onClick={() => setExpanded(!expanded)}
              className="text-body cursor-pointer self-start px-(--space-2) font-medium text-(--accent)"
            >
              {expanded ? "View less" : "View all"}
            </button>
          ) : null}
        </div>
      )}
    </section>
  );
}

export function ProjectsFilterPanel({
  filters,
  members,
  selfId,
  onToggleMyProjects,
  onToggleAccess,
  onTogglePerson,
  onSelectCreated,
}: ProjectsFilterPanelProps): React.ReactElement {
  const [valueSearch, setValueSearch] = React.useState("");
  const [collapsed, setCollapsed] = React.useState<Record<SectionKey, boolean>>({
    access: false,
    lead: false,
    members: false,
    created: false,
  });
  const [customOpen, setCustomOpen] = React.useState(false);

  const toggleSection = (section: SectionKey): void =>
    setCollapsed((current) => ({ ...current, [section]: !current[section] }));

  const needle = valueSearch.toLowerCase();
  const accessOptions = ACCESS_OPTIONS.filter((option) => option.label.toLowerCase().includes(needle));
  const createdOptions = CREATED_PRESETS.filter((option) => option.label.toLowerCase().includes(needle));
  const customSelected = filters.createdAt.some((value) => isCustomCreatedValue(value));
  const badge = countActiveListFilters(filters) > 0;

  return (
    <Popover
      trigger={
        <Button variant="secondary" size="small" data-testid="projects-filter-trigger">
          <ListFilter size={14} strokeWidth={2} aria-hidden="true" />
          <span>Filters</span>
          {badge ? (
            <span
              data-testid="projects-filter-badge"
              aria-hidden="true"
              className="size-1.5 rounded-full bg-(--accent)"
            />
          ) : null}
        </Button>
      }
      className="max-h-[24rem] w-72 overflow-y-auto"
    >
      <div data-testid="projects-filter-panel" className="flex flex-col gap-(--space-2)">
        <div className="flex items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) py-(--space-2)">
          <Search size={14} strokeWidth={2} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
          <input
            type="text"
            aria-label="Search filters"
            placeholder="Search"
            value={valueSearch}
            onChange={(event) => setValueSearch(event.target.value)}
            className="text-body w-full bg-transparent outline-none placeholder:text-(--text-muted)"
          />
          {valueSearch === "" ? null : (
            <button
              type="button"
              aria-label="Clear filter search"
              onClick={() => setValueSearch("")}
              className="grid cursor-pointer place-items-center text-(--text-muted)"
            >
              <X size={14} strokeWidth={2} aria-hidden="true" />
            </button>
          )}
        </div>

        <label className="text-body flex cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) hover:bg-(--subtle)">
          <input type="checkbox" checked={filters.myProjects} onChange={onToggleMyProjects} />
          My projects
        </label>

        <section aria-label="Access">
          <SectionHeader
            title="Access"
            count={filters.access.length}
            collapsed={collapsed.access}
            onToggle={() => toggleSection("access")}
          />
          {collapsed.access ? null : (
            <div className="flex flex-col gap-(--space-1)">
              {accessOptions.length === 0 ? (
                <p className="text-body text-(--text-muted) italic">No matches found</p>
              ) : (
                accessOptions.map((option) => (
                  <label
                    key={option.value}
                    className="text-body flex cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) hover:bg-(--subtle)"
                  >
                    <input
                      type="checkbox"
                      checked={filters.access.includes(option.value)}
                      onChange={() => onToggleAccess(option.value)}
                    />
                    {option.value === 0 ? (
                      <Lock size={14} strokeWidth={2} aria-hidden="true" />
                    ) : (
                      <Globe size={14} strokeWidth={2} aria-hidden="true" />
                    )}
                    {option.label}
                  </label>
                ))
              )}
            </div>
          )}
        </section>

        <PersonSection
          title="Lead"
          dimension="lead"
          applied={filters.lead}
          memberships={members}
          selfId={selfId}
          query={valueSearch}
          collapsed={collapsed.lead}
          onToggleSection={() => toggleSection("lead")}
          onTogglePerson={onTogglePerson}
        />

        <PersonSection
          title="Members"
          dimension="members"
          applied={filters.members}
          memberships={members}
          selfId={selfId}
          query={valueSearch}
          collapsed={collapsed.members}
          onToggleSection={() => toggleSection("members")}
          onTogglePerson={onTogglePerson}
        />

        <section aria-label="Created date">
          <SectionHeader
            title="Created date"
            count={filters.createdAt.length}
            collapsed={collapsed.created}
            onToggle={() => toggleSection("created")}
          />
          {collapsed.created ? null : (
            <div className="flex flex-col gap-(--space-1)">
              {createdOptions.length === 0 ? (
                <p className="text-body text-(--text-muted) italic">No matches found</p>
              ) : (
                <>
                  {createdOptions.map((option) => {
                    const selected = filters.createdAt.includes(option.value);
                    return (
                      <label
                        key={option.value}
                        className="text-body flex cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) hover:bg-(--subtle)"
                      >
                        <input
                          type="checkbox"
                          checked={selected}
                          onChange={() => onSelectCreated(selected ? [] : [option.value])}
                        />
                        {option.label}
                      </label>
                    );
                  })}
                  <label className="text-body flex cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) hover:bg-(--subtle)">
                    <input
                      type="checkbox"
                      checked={customSelected}
                      onChange={() => {
                        if (customSelected) onSelectCreated([]);
                        else setCustomOpen(true);
                      }}
                    />
                    Custom
                  </label>
                </>
              )}
            </div>
          )}
        </section>
      </div>
      <ProjectsCustomDateDialog
        open={customOpen}
        onOpenChange={setCustomOpen}
        onApply={(values) => onSelectCreated(values)}
      />
    </Popover>
  );
}
