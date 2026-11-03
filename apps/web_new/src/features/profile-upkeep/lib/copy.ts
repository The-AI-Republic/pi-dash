// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Wording for the automatic-project-upkeep tab (parity/specs/profile.md
// §11, PROF-024). The empty-state line and the failure notice are pinned
// by the oracle; the rest is new prose for this app.

export const UPKEEP_TAB_HEADING = "Auto Project Management";
export const UPKEEP_TAB_INTRO =
  "Routine upkeep runs automatically with your permissions, and only inside the workspaces you belong to.";

export const UPKEEP_EMPTY_EXPLANATION = "Nothing is scheduled on this instance yet.";

export const MASTER_PAUSE_TITLE = "Pause all Auto Project Management";
export const MASTER_PAUSE_DESCRIPTION = "Stops every job without changing the individual switches below.";

export const UPKEEP_FAILED_TITLE = "Something went wrong";
export const UPKEEP_FAILED_FALLBACK = "Please try again.";
