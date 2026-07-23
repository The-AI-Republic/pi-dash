/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { RANDOM_EMOJI_CODES } from "@pi-dash/constants";
import type { IProject } from "@pi-dash/types";
import { getRandomCoverImage } from "@/helpers/cover-image.helper";

// Detect the user's local IANA timezone from the browser so new projects
// default to it instead of always defaulting to UTC. Modern browsers
// canonicalize the value returned by `resolvedOptions().timeZone`
// (e.g. "Asia/Calcutta" -> "Asia/Kolkata"), so it lines up with the backend's
// `pytz.common_timezones` choices. If detection is unavailable or throws, fall
// back to UTC.
const getBrowserTimezone = (): string => {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
  } catch {
    return "UTC";
  }
};

export const getProjectFormValues = (): Partial<IProject> => ({
  cover_image_url: getRandomCoverImage(),
  timezone: getBrowserTimezone(),
  description: "",
  logo_props: {
    in_use: "emoji",
    emoji: {
      value: RANDOM_EMOJI_CODES[Math.floor(Math.random() * RANDOM_EMOJI_CODES.length)],
    },
  },
  identifier: "",
  name: "",
  network: 2,
  project_lead: null,
  repo_url: "",
  base_branch: "",
  // MVP: new projects start with work items only; users can re-enable
  // cycles/modules/views/pages/intake later from project settings.
  cycle_view: false,
  module_view: false,
  issue_views_view: false,
  page_view: false,
  inbox_view: false,
});
