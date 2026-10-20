// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Attach existing work items to the cycle or module a page is scoped to
// (parity/specs/issues.md §2, ISS-012; §8, ISS-069/070). The group headers
// and the empty cards reach the same search modal, and both hand their
// picks here: the hook writes the bridge, notifies, and refreshes the
// entity's lists. A page with neither scope has nothing to attach to, so
// the hook reports that instead of silently succeeding.

import { useQueryClient } from "@tanstack/react-query";
import { addIssueToCycle, addIssuesToModule } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { issueKeys } from "../api/keys.js";
import { entityListsKeyPart, type IssuesEntity } from "../prefs/entities.js";

/** Failure notice shared by every attach entry. */
export const ATTACH_FAILED_TITLE = "Couldn't add the work items";
const ATTACH_FAILED_BODY = "Check your connection and try again.";

export interface AttachExistingInput {
  workspaceSlug: string;
  projectId: string;
  entity: IssuesEntity;
  cycleId?: string | undefined;
  moduleId?: string | undefined;
}

/**
 * Write one pick set onto the page's cycle or module. Rows already in the
 * target are filtered by the search modal; this side only writes. The
 * result tells the caller whether the modal may close: a failure keeps it
 * open so the picks are not lost.
 */
export function useAttachExisting({ workspaceSlug, projectId, entity, cycleId, moduleId }: AttachExistingInput) {
  const queryClient = useQueryClient();
  const [isAttaching, setAttaching] = React.useState(false);
  const attach = React.useCallback(
    async (ids: string[]): Promise<boolean> => {
      const client = getAppClient();
      setAttaching(true);
      try {
        if (cycleId !== undefined) {
          for (const id of ids) await addIssueToCycle(client, workspaceSlug, projectId, cycleId, id);
        } else if (moduleId !== undefined) {
          await addIssuesToModule(client, workspaceSlug, projectId, moduleId, ids);
        } else {
          throw new Error("this page has no cycle or module to attach to");
        }
        notifySuccess(ids.length === 1 ? "Work item added" : "Work items added");
        await queryClient.invalidateQueries({
          queryKey: issueKeys.lists(workspaceSlug, ...entityListsKeyPart(entity)),
        });
        return true;
      } catch {
        notifyError(ATTACH_FAILED_TITLE, ATTACH_FAILED_BODY);
        return false;
      } finally {
        setAttaching(false);
      }
    },
    [cycleId, entity, moduleId, projectId, queryClient, workspaceSlug]
  );
  return { attach, isAttaching };
}
