// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Driver factory (NEWFRONT-19). Picks the driver from PARITY_TARGET so one
// scenario file runs against either frontend without edits.
import type { Page } from "@playwright/test";
import type { ParityDriver, ParityTarget } from "./parity-driver";
import { WebDriver } from "./web";
import { WebNewDriver } from "./web-new";

export type { ParityDriver, ParityTarget } from "./parity-driver";
export { WEB_TEST_IDS } from "./web";

export function parityTargetFromEnv(): ParityTarget {
  const raw = (process.env["PARITY_TARGET"] ?? "web").trim();
  if (raw === "web" || raw === "web_new") return raw;
  throw new Error(`[parity] PARITY_TARGET must be "web" or "web_new", got ${JSON.stringify(raw)}.`);
}

export function createDriver(page: Page, target: ParityTarget): ParityDriver {
  if (target === "web_new") return new WebNewDriver(page);
  return new WebDriver(page);
}
