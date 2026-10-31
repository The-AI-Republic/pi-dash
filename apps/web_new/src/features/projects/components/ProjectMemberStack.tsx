// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project member stack (SHELL-032). Overlapping avatars with an
// overflow count: small teams render whole, larger ones collapse past
// the second avatar into a "+N" bubble.
import { Avatar } from "@pidash/kit";
import * as React from "react";

export interface MemberStackEntry {
  id: string;
  name: string;
  avatarUrl: string | null;
}

/** Avatars kept once the stack overflows; small teams render whole. */
export const MEMBER_STACK_CAP = 2;

export interface ProjectMemberStackProps {
  /** One entry per project member, in stack order. */
  members: MemberStackEntry[];
}

/** Visible avatars plus the overflow remainder for a team of `total`. */
export function selectStackSplit(total: number): { shown: number; overflow: number } {
  if (total <= MEMBER_STACK_CAP + 1) return { shown: total, overflow: 0 };
  return { shown: MEMBER_STACK_CAP, overflow: total - MEMBER_STACK_CAP };
}

export function ProjectMemberStack({ members }: ProjectMemberStackProps): React.ReactElement {
  const total = members.length;
  if (total === 0) {
    return (
      <span data-testid="project-member-stack-empty" className="text-body text-(--text-muted) italic">
        No members yet
      </span>
    );
  }
  const { shown, overflow } = selectStackSplit(total);
  return (
    <span
      role="group"
      aria-label={`${total} ${total === 1 ? "member" : "members"}`}
      data-testid="project-member-stack"
      className="flex items-center"
    >
      {members.slice(0, shown).map((member, index) => (
        <Avatar
          key={member.id}
          name={member.name}
          {...(member.avatarUrl === null ? {} : { src: member.avatarUrl })}
          size="small"
          className={index === 0 ? "ring-2 ring-(--surface)" : "-ml-2 ring-2 ring-(--surface)"}
        />
      ))}
      {overflow > 0 ? (
        <span
          aria-label={`${overflow} more members`}
          data-testid="project-member-stack-overflow"
          className="text-caption -ml-2 grid size-6 shrink-0 place-items-center rounded-(--radius-full) bg-(--subtle) font-medium text-(--text-muted) ring-2 ring-(--surface)"
        >
          +{overflow}
        </span>
      ) : null}
    </span>
  );
}
