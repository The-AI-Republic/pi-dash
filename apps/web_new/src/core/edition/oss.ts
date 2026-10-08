// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The OSS edition: no extra routes, sidebar entries, settings sections,
// auth providers, middleware, or slots. The cloud build replaces this
// module through the @pidash/edition alias.

import type { Edition } from "./types.js";

export const edition: Edition = {
  id: "oss",
  flags: {},
};
