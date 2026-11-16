// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The thin project projection embedded in members and activity rows. It
// lives apart from projects.ts so the lazy settings contracts can use it
// without making the shell's eager project module carry (and export) it
// (size gate, NEWFRONT-579).
import * as z from "zod/mini";
import { uuid } from "./fields.js";

/** Small project projection embedded in members and activity. */
export const ProjectLite = z.object({
  id: uuid(),
  name: z.string(),
  identifier: z.string(),
});
export type ProjectLite = z.infer<typeof ProjectLite>;
