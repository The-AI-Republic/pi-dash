// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  ADMIN_ENTRY_HREF,
  appInstanceQueryOptions,
  decideAppInstanceGate,
  InstanceAppGate,
  instanceAppKeys,
  MAINTENANCE_HEADING,
  MaintenanceScreen,
  NotReadyScreen,
  PRODUCT_NAME,
} from "./index.js";

describe("instance feature barrel", () => {
  it("exports the gate, screens, query policy and branding used by the app entry", () => {
    expect(InstanceAppGate).toBeTypeOf("function");
    expect(MaintenanceScreen).toBeTypeOf("function");
    expect(NotReadyScreen).toBeTypeOf("function");
    expect(decideAppInstanceGate({ isPending: true, isError: false, data: undefined })).toBe("loading");
    expect(instanceAppKeys.bootstrap).toEqual(["instance", "app", "bootstrap"]);
    expect(appInstanceQueryOptions).toBeTypeOf("function");
    expect(MAINTENANCE_HEADING).toContain("didn't start up correctly");
    expect(ADMIN_ENTRY_HREF).toBe("/god-mode/");
    expect(PRODUCT_NAME).toBe("Pi Dash");
  });
});
