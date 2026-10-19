// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Today's-activity download (parity/specs/profile.md §3, PROF-007). Posts
// the local day, shows a working label until the server's bytes arrive,
// then saves them under a timestamped name. The caller only mounts this
// for workspace admins and members, so guests never see the control.

import { Button } from "@pidash/kit";
import { Download } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError } from "../../../shared/shell/index.js";
import { PROFILE_EXPORT_FAILED_TITLE, downloadProfileActivity } from "../api/export.js";

export const PROFILE_DOWNLOAD_IDLE_LABEL = "Download today's activity";
export const PROFILE_DOWNLOAD_WORKING_LABEL = "Downloading";

export interface ProfileActivityDownloadProps {
  workspaceSlug: string;
  userId: string;
  /** Injected for tests; defaults to the real fetch-and-save. */
  run?: () => Promise<unknown>;
}

export function ProfileActivityDownload({
  workspaceSlug,
  userId,
  run,
}: ProfileActivityDownloadProps): React.ReactElement {
  const [downloading, setDownloading] = React.useState(false);
  const start = async (): Promise<void> => {
    setDownloading(true);
    try {
      await (run ?? (() => downloadProfileActivity(getAppClient(), workspaceSlug, userId)))();
    } catch (error) {
      notifyError(PROFILE_EXPORT_FAILED_TITLE, error instanceof Error ? error.message : "Try again in a moment.");
    } finally {
      setDownloading(false);
    }
  };
  return (
    <Button
      variant="secondary"
      size="small"
      data-testid="profile-activity-download"
      disabled={downloading}
      onClick={() => {
        void start();
      }}
    >
      <Download size={14} strokeWidth={1.5} aria-hidden="true" />
      <span>{downloading ? PROFILE_DOWNLOAD_WORKING_LABEL : PROFILE_DOWNLOAD_IDLE_LABEL}</span>
    </Button>
  );
}
