// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Resolving peek mount (parity/specs/issues.md §14, ISS-165/167). The
// layouts mount this with the raw peeked value: a list key resolves
// against the rows the opener holds, a raw id reads under the peeked
// (or route) project, and an unmatched key resolves through the lite
// read first — so deep links open whatever the list has loaded. The lite
// wait and every failure reuse the shared status panels.
//
// A nested child replaces the address with its own issue, so the mount
// latches the panel the layout opened: while the address names the child
// the root panel stays put, and a child close returns the address to it.
// Recognising that child needs the host's address basis (the project its
// writer omits), which is not always the project the panel reads under.
// Hosts that own the address receive that selection (issue, project,
// nesting level) through onPeekChange; layouts without URL sync leave it
// undefined and the stack stays local.

import { useQuery } from "@tanstack/react-query";
import type { IssueListItem } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { liteIssueQueryOptions } from "../api/browse.js";
import { IssuePeek } from "./IssuePeek.js";
import { PeekErrorPanel, PeekLoadingPanel } from "./peekStates.js";
import { resolvePeekTarget, type PeekTarget } from "./peekTarget.js";
import { peekNamesChild, peekNamesRoot, type PeekRoot, type PeekSelection } from "./peekUrl.js";

export interface ResolvingIssuePeekProps {
  workspaceSlug: string;
  /** Project of the mounting route. */
  projectId: string;
  /** Code the opener keyed its rows with. */
  projectIdentifier: string;
  /** The raw peeked value: list key or issue id. */
  peeked: string;
  /** Peeked project from the URL, when it differs from the route. */
  peekProjectId?: string | null | undefined;
  /**
   * Project the host's writer treats as its own when it omits the param.
   * Defaults to the route project; a surface with no route project (the
   * workspace all-issues list) passes "" because it spells the project
   * on every write.
   */
  addressProjectId?: string | undefined;
  /** Rows the opener holds for key resolution without a fetch. */
  rows?: readonly Pick<IssueListItem, "id" | "project_id" | "sequence_id">[] | undefined;
  /** Receives the address a nested open or child close implies (ISS-167). */
  onPeekChange?: ((selection: PeekSelection) => void) | undefined;
  onClose: () => void;
}

/** Remount the root panel when the latched panel itself changes. */
function mountKey(root: PeekRoot): string {
  return `${root.peeked}\u0000${root.peekProjectId ?? ""}`;
}

export function ResolvingIssuePeek({
  workspaceSlug,
  projectId,
  projectIdentifier,
  peeked,
  peekProjectId = null,
  addressProjectId = projectId,
  rows = [],
  onPeekChange,
  onClose,
}: ResolvingIssuePeekProps): React.ReactElement {
  const client = getAppClient();
  const [root, setRoot] = React.useState<PeekRoot>({ peeked, peekProjectId });
  // The guard below must see a child the moment it opens: the address
  // update and the stack update land in the same batch, and the address
  // may settle first.
  const childRef = React.useRef<PeekSelection | null>(null);

  React.useEffect(() => {
    if (peekNamesRoot(root, peeked, peekProjectId)) return;
    if (peekNamesChild(childRef.current, addressProjectId, peeked, peekProjectId)) return;
    // The address moved somewhere this mount did not open: a browser Back,
    // a shared link, or another list. Drop the stack and follow it.
    childRef.current = null;
    setRoot({ peeked, peekProjectId });
  }, [addressProjectId, peeked, peekProjectId, root]);

  const known = resolvePeekTarget(root.peeked, {
    rows,
    projectIdentifier,
    routeProjectId: projectId,
    peekProjectId: root.peekProjectId,
  });
  const lite = useQuery({
    ...liteIssueQueryOptions(client, workspaceSlug, root.peeked.trim()),
    enabled: known === null,
  });
  const liteRow = lite.data?.lite ?? null;
  const resolved: PeekTarget | null =
    known ??
    (liteRow === null ? null : { issueId: liteRow.id, projectId: liteRow.project_id, issueKey: root.peeked.trim() });

  const handleNested = React.useCallback(
    (selection: PeekSelection): void => {
      // The address now names this rung or a deeper one; the root stays.
      childRef.current = selection;
      onPeekChange?.(selection);
    },
    [onPeekChange]
  );

  if (known === null && lite.isPending) return <PeekLoadingPanel mode="side" onClose={onClose} />;
  if (resolved === null) return <PeekErrorPanel mode="side" onClose={onClose} />;
  return (
    <IssuePeek
      key={mountKey(root)}
      workspaceSlug={workspaceSlug}
      projectId={resolved.projectId}
      projectIdentifier={projectIdentifier}
      issueId={resolved.issueId}
      issueKey={resolved.issueKey}
      onNestedChange={handleNested}
      onClose={onClose}
    />
  );
}
