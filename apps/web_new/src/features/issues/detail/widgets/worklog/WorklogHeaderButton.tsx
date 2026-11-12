// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Worklog create control (parity/specs/issues.md §16, ISS-206, cloud
// leg). The first button in the activity header's control row, opening
// the log-time dialog; a successful save closes the dialog, while a
// refused one keeps it open with the values intact. The parent gates
// the mount on eligibility — this button always renders when mounted.

import { IconButton } from "@pidash/kit";
import { Timer } from "lucide-react";
import * as React from "react";

import { useLogWorklog } from "./api.js";
import { WORKLOG_LOG_ACTION } from "./format.js";
import { WorklogDialog } from "./WorklogDialog.js";

export interface WorklogHeaderButtonProps {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
}

export function WorklogHeaderButton({
  workspaceSlug,
  projectId,
  issueId,
}: WorklogHeaderButtonProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const log = useLogWorklog(workspaceSlug, projectId, issueId);

  return (
    <>
      <IconButton
        label={WORKLOG_LOG_ACTION}
        onClick={() => {
          setOpen(true);
        }}
      >
        <Timer size={16} strokeWidth={1.5} aria-hidden="true" />
      </IconButton>
      {open ? (
        <WorklogDialog
          saving={log.isPending}
          onClose={() => {
            setOpen(false);
          }}
          onSubmit={(input) => {
            log.mutate(input, {
              onSuccess: () => {
                setOpen(false);
              },
            });
          }}
        />
      ) : null}
    </>
  );
}
