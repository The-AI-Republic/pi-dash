// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Settings restriction views (PSET-003, from
// parity/specs/project-settings-shell.md). One view per denial kind: a
// join prompt that lands the viewer back on the same settings address, a
// contact-an-admin notice without an action, and the not-found notice an
// unknown id falls through to.

import { Button } from "@pidash/kit";
import * as React from "react";

export const RESTRICTION_NOT_FOUND_TITLE = "Project not found";
export const RESTRICTION_NOT_FOUND_BODY = "This project does not exist, or the address is wrong.";
export const RESTRICTION_JOIN_TITLE = "You don’t have access to this project yet";
export const RESTRICTION_JOIN_BODY = "Join the project to open its settings.";
export const RESTRICTION_NO_ACCESS_TITLE = "You can’t open this project’s settings";
export const RESTRICTION_NO_ACCESS_BODY = "This project is private. Ask a workspace admin for access.";

export type ProjectSettingsRestrictionKind = "join" | "no-access" | "not-found";

export interface ProjectSettingsRestrictionProps {
  kind: ProjectSettingsRestrictionKind;
  joining?: boolean;
  onJoin?: () => void;
}

function RestrictionShell({
  kind,
  heading,
  body,
  action,
}: {
  kind: ProjectSettingsRestrictionKind;
  heading: string;
  body: string;
  action?: React.ReactNode | undefined;
}): React.ReactElement {
  return (
    <section
      aria-label={heading}
      data-testid="project-settings-restriction"
      data-restriction={kind}
      className="flex h-full min-h-64 w-full flex-col items-center justify-center gap-(--space-2) p-(--space-12) text-center"
    >
      <h1 className="text-h3 text-(--text)">{heading}</h1>
      <p className="text-body max-w-md text-(--text-muted)">{body}</p>
      {action === undefined ? null : <div className="pt-(--space-2)">{action}</div>}
    </section>
  );
}

/** Not-authorized body for a page gate refusal; the shell keeps its rail. */
export function ProjectSettingsNotAuthorized({ heading }: { heading: string }): React.ReactElement {
  return (
    <section
      aria-label={heading}
      data-testid="project-settings-denied"
      className="flex flex-col gap-(--space-6) p-(--space-8)"
    >
      <h1 className="text-h2 text-(--text)">Not authorized</h1>
      <div className="flex flex-col items-center gap-(--space-2) py-(--space-12) text-center">
        <h2 className="text-h3 text-(--text)">Not authorized to view {heading.toLowerCase()}</h2>
        <p className="text-body text-(--text-muted)">Your role in this project cannot open this page.</p>
      </div>
    </section>
  );
}

export function ProjectSettingsRestriction({
  kind,
  joining = false,
  onJoin,
}: ProjectSettingsRestrictionProps): React.ReactElement {
  if (kind === "join") {
    return (
      <RestrictionShell
        kind={kind}
        heading={RESTRICTION_JOIN_TITLE}
        body={RESTRICTION_JOIN_BODY}
        action={
          <Button variant="primary" loading={joining} disabled={joining} onClick={onJoin}>
            {joining ? "Joining…" : "Join project"}
          </Button>
        }
      />
    );
  }
  if (kind === "no-access") {
    return <RestrictionShell kind={kind} heading={RESTRICTION_NO_ACCESS_TITLE} body={RESTRICTION_NO_ACCESS_BODY} />;
  }
  return <RestrictionShell kind={kind} heading={RESTRICTION_NOT_FOUND_TITLE} body={RESTRICTION_NOT_FOUND_BODY} />;
}
