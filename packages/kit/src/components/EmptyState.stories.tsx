// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { EmptyState } from "./EmptyState";

const noop = (): void => {};

export const Illustrated = (): React.ReactElement => (
  <EmptyState
    variant="illustrated"
    title="No work items found"
    description="Try a different search"
    art={{ category: "general", type: "no-results" }}
  />
);

export const IllustratedDark = (): React.ReactElement => (
  <div data-theme="dark">
    <EmptyState
      variant="illustrated"
      title="No work items found"
      description="Try a different search"
      art={{ category: "general", type: "no-results" }}
      scheme="dark"
    />
  </div>
);

export const Marketing = (): React.ReactElement => (
  <EmptyState
    variant="marketing"
    title="Welcome to cycles"
    description="Plan work in time-boxed iterations"
    art={{ category: "general", type: "empty" }}
    primaryAction={{ label: "Create a cycle", onSelect: noop }}
    hint="Cycles group work by week or sprint"
  />
);

export const Detailed = (): React.ReactElement => (
  <EmptyState
    variant="detailed"
    title="Cycles is not enabled for this project"
    description="Enable the cycles feature to plan iterations"
    art={{ category: "general", type: "disabled" }}
    primaryAction={{ label: "Manage features", onSelect: noop }}
    secondaryAction={{ label: "Learn more", onSelect: noop }}
  />
);

export const DetailedNoActions = (): React.ReactElement => (
  <EmptyState
    variant="detailed"
    title="Nothing to show"
    description="Items will appear here"
    art={{ category: "general", type: "empty" }}
  />
);

export const Section = (): React.ReactElement => (
  <EmptyState
    variant="section"
    title="Updates paused"
    description="Resume any time to keep receiving them"
    action={{ label: "Resume", onSelect: noop }}
  />
);
