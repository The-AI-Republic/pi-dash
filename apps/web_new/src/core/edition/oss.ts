// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The OSS edition: no extra routes, sidebar entries, settings sections,
// middleware, or slots. The cloud build replaces this module through the
// @pidash/edition alias. The four native providers are listed (not cloud
// extras): the backend serves their auth URLs in every build, so the
// instance flags alone decide which buttons show (AUTH-003 parity: the
// old app offers each instance-enabled provider on OSS too).

import type { Edition } from "./types.js";

export const edition: Edition = {
  id: "oss",
  flags: {},
  auth: [
    { id: "google", label: "Google" },
    { id: "github", label: "GitHub" },
    { id: "gitlab", label: "GitLab" },
    { id: "gitea", label: "Gitea" },
  ],
};
