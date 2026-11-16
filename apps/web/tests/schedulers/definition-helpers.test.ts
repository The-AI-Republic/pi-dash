/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { describe, expect, it } from "vitest";
import { deriveSchedulerSlug } from "@/components/schedulers/definition-helpers";
// Shared with the server's derive_scheduler_slug (and the Rust port) so the
// client and server rules can't drift apart.
import vectors from "../../../api/pi_dash/scheduler/slug_vectors.json";

describe("deriveSchedulerSlug", () => {
  it("has shared vectors to check", () => {
    expect(vectors.length).toBeGreaterThan(10);
  });

  it.each(vectors.map((v) => [v.name, v.slug] as const))("derives %j -> %j", (name, slug) => {
    expect(deriveSchedulerSlug(name)).toBe(slug);
  });
});
