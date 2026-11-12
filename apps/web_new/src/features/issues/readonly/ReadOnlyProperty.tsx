// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Read-only property values (parity/specs/issues.md §17, ISS-220): icon,
// value, "None" fallback, never a trigger. Archived and guest surfaces
// use these where the layout wants a value with no affordance; rows
// keeping their control for accessibility use a disabled picker.

import { Avatar, Tooltip } from "@pidash/kit";
import { Calendar, Circle, CircleCheck, CircleDot, Flag, Gauge, Hash, Tags, UserCircle2, Users } from "lucide-react";
import * as React from "react";

/** The value a property shows when it has nothing to say. */
export const READONLY_NONE = "None";

/** The icon each property kind renders, and whether it is filled. */
export const READONLY_PROPERTY_ICONS = {
  state: CircleDot,
  priority: Flag,
  assignee: Users,
  labels: Tags,
  date: Calendar,
  estimate: Gauge,
  cycle: Circle,
  module: Circle,
  parent: Hash,
  creator: UserCircle2,
  completed: CircleCheck,
} as const;

export type ReadOnlyPropertyKind = keyof typeof READONLY_PROPERTY_ICONS;

export interface ReadOnlyPropertyProps {
  kind: ReadOnlyPropertyKind;
  /** The value text; null, undefined or blank renders "None". */
  value: string | null | undefined;
  /** Optional hover text, e.g. the full label names behind a count. */
  tooltip?: string | undefined;
  /** Extra classes for the value row. */
  className?: string | undefined;
}

/** The visible value, defaulting blanks to "None". */
export function readOnlyValueText(value: string | null | undefined): string {
  if (value === null || value === undefined) return READONLY_NONE;
  const trimmed = value.trim();
  return trimmed === "" ? READONLY_NONE : trimmed;
}

/** One non-interactive property value: icon plus text, never a trigger. */
export function ReadOnlyProperty({ kind, value, tooltip, className }: ReadOnlyPropertyProps): React.ReactElement {
  const Icon = READONLY_PROPERTY_ICONS[kind];
  const text = readOnlyValueText(value);
  const body = (
    <span
      data-readonly-property={kind}
      className={className ?? "text-body inline-flex min-w-0 items-center gap-(--space-1) text-(--text)"}
    >
      <Icon size={14} strokeWidth={1.5} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
      <span className={text === READONLY_NONE ? "truncate text-(--text-muted)" : "truncate"}>{text}</span>
    </span>
  );
  if (tooltip === undefined || tooltip === "") return body;
  return <Tooltip content={tooltip}>{body}</Tooltip>;
}

/** Label names behind a count, for the Labels row's tooltip. */
export function labelNamesTooltip(names: readonly string[]): string | undefined {
  return names.length === 0 ? undefined : names.join(", ");
}

/** Count rendering for a multi-value property: "3 labels", "1 label". */
export function countLabel(count: number, singular: string, plural: string = `${singular}s`): string {
  return `${count} ${count === 1 ? singular : plural}`;
}

export interface ReadOnlyMembersProps {
  members: readonly { id: string; name: string }[];
  /** How many avatars render before a "+N" count takes over. */
  max?: number | undefined;
}

/** Members as an avatar group plus a name summary; "None" when empty. */
export function ReadOnlyMembers({ members, max = 3 }: ReadOnlyMembersProps): React.ReactElement {
  if (members.length === 0) return <ReadOnlyProperty kind="assignee" value={null} />;
  const shown = members.slice(0, max);
  const extra = members.length - shown.length;
  return (
    <span
      data-readonly-property="assignee"
      className="text-body inline-flex min-w-0 items-center gap-(--space-1) text-(--text)"
    >
      {shown.map((member) => (
        <Avatar key={member.id} name={member.name} size="small" />
      ))}
      {extra > 0 ? <span className="text-meta text-(--text-muted)">+{extra}</span> : null}
      <span className="truncate">{members.map((member) => member.name).join(", ")}</span>
    </span>
  );
}

export interface ReadOnlyLabelsProps {
  names: readonly string[];
}

/** Labels as a count with the names on hover; "None" when unlabeled. */
export function ReadOnlyLabels({ names }: ReadOnlyLabelsProps): React.ReactElement {
  return (
    <ReadOnlyProperty
      kind="labels"
      value={names.length === 0 ? null : countLabel(names.length, "label")}
      tooltip={labelNamesTooltip(names)}
    />
  );
}

export interface ReadOnlyDateProps {
  /** ISO day, or null for unset. */
  iso: string | null | undefined;
}

/** A calendar day rendered as the date's own ISO text; "None" when unset. */
export function ReadOnlyDate({ iso }: ReadOnlyDateProps): React.ReactElement {
  return <ReadOnlyProperty kind="date" value={iso ?? null} />;
}
