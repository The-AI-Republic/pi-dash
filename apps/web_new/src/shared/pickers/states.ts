// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// State option shaping (parity/specs/issues.md §17, ISS-209). The picker
// lists the project's workflow states in server order with a per-group
// glyph tinted by the state's own color. There is no clear row: a state
// is always set.
import type { LucideIcon } from "lucide-react";
import { Circle, CircleCheck, CircleDashed, CircleX, Contrast, Eye, FlaskConical, Inbox } from "lucide-react";

import type { State, StateList } from "@pidash/api-client";

export type StateGroup = State["group"];

/** Workflow-group glyphs; rendered in the state's own color. */
export const STATE_GROUP_ICONS: Record<StateGroup, LucideIcon> = {
  backlog: CircleDashed,
  unstarted: Circle,
  started: Contrast,
  review: Eye,
  test: FlaskConical,
  completed: CircleCheck,
  cancelled: CircleX,
  triage: Inbox,
};

/** One state row in server order. */
export interface StateOption {
  id: string;
  name: string;
  group: StateGroup;
  color: string;
}

export function selectStateOptions(states: StateList): StateOption[] {
  return states.map((state) => ({ id: state.id, name: state.name, group: state.group, color: state.color }));
}
