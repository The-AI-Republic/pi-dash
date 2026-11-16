// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project access wall (AUTH-024, parity/specs/modules.md §8.1, §10.3,
// MOD-038). One block stands between a visitor and a project they may not
// read: join where the server allows it, an explanation where it does
// not, and a not-found branch for addresses that name no project. The
// project shell renders it, and so does every project area that can meet
// a refusal while the shell itself still resolves — the modules detail
// gate is the first. Areas pass a kind and, for the join wall, the join
// action; the copy and the focus handling stay here so the wall cannot
// drift into two shapes.

import { Button } from "@pidash/kit";
import * as React from "react";

import { authStepInputProps } from "../focus.js";

/** Which wall a refusal maps to. Mirrors `decideProjectDenial`. */
export type ProjectAccessWallKind = "join" | "no-access" | "not-found";

export interface ProjectAccessWallProps {
  kind: ProjectAccessWallKind;
  /** Join handler; required for the join wall, ignored otherwise. */
  onJoin?: (() => void) | undefined;
  /** True while the join request is in flight. */
  joining?: boolean | undefined;
}

const COPY: Record<ProjectAccessWallKind, { heading: string; body: string }> = {
  join: {
    heading: "You haven't joined this project yet",
    body: "Join it to see its work.",
  },
  "no-access": {
    heading: "You can't open this project",
    body: "This project is private. Ask a workspace admin for access to continue.",
  },
  "not-found": {
    heading: "Project not found",
    body: "It may have been deleted, or the address is wrong.",
  },
};

export function ProjectAccessWall({ kind, onJoin, joining = false }: ProjectAccessWallProps): React.ReactElement {
  const copy = COPY[kind];
  const action =
    kind !== "join" || onJoin === undefined ? null : (
      <Button variant="primary" loading={joining} disabled={joining} {...authStepInputProps()} onClick={() => onJoin()}>
        {joining ? "Joining…" : "Join project"}
      </Button>
    );
  return (
    <section
      aria-label={copy.heading}
      data-testid="project-access-wall"
      data-wall-kind={kind}
      className="flex h-full w-full flex-col items-center justify-center gap-(--space-2) p-(--space-12) text-center"
    >
      <h2 className="text-h3 text-(--text)">{copy.heading}</h2>
      <p className="text-body max-w-md text-(--text-muted)">{copy.body}</p>
      {action === null ? null : <div className="pt-(--space-2)">{action}</div>}
    </section>
  );
}
