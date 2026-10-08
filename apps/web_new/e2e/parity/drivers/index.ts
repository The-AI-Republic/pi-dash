// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Driver factory (NEWFRONT-19). Picks the driver from PARITY_TARGET so one
// scenario file runs against either frontend without edits.
import type { Page } from "@playwright/test";
import type { ParityDriver, ParityTarget } from "./parity-driver";
import { WebDriver } from "./web";
import { WebNewDriver } from "./web-new";

export type { ParityDriver, ParityTarget } from "./parity-driver";

export function parityTargetFromEnv(): ParityTarget {
  const raw = (process.env["PARITY_TARGET"] ?? "web").trim();
  if (raw === "web" || raw === "web_new") return raw;
  throw new Error(`[parity] PARITY_TARGET must be "web" or "web_new", got ${JSON.stringify(raw)}.`);
}

export function createDriver(page: Page, target: ParityTarget): ParityDriver {
  if (target === "web_new") return new WebNewDriver(page);
  return new WebDriver(page);
}
