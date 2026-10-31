// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The script under test, resolved for the subprocess entry-point test.
import { fileURLToPath } from "node:url";

export const PRERENDER_ENTRY = fileURLToPath(new URL("./prerender.mjs", import.meta.url));
