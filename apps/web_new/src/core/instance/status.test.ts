// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { InstanceBootstrapState } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { decideAppInstanceGate } from "./status.js";

const READY: InstanceBootstrapState = { setupDone: true, bootstrap: null };
const NOT_READY: InstanceBootstrapState = { setupDone: false, bootstrap: null };

describe("decideAppInstanceGate", () => {
  it("holds the loader while the read is unresolved", () => {
    expect(decideAppInstanceGate({ isPending: true, isError: false, data: undefined })).toBe("loading");
  });

  it("shows maintenance when the read fails", () => {
    expect(decideAppInstanceGate({ isPending: false, isError: true, data: undefined })).toBe("maintenance");
  });

  it("settles on maintenance rather than loading once nothing is pending", () => {
    expect(decideAppInstanceGate({ isPending: false, isError: false, data: undefined })).toBe("maintenance");
  });

  it("prefers settled data over an error status", () => {
    expect(decideAppInstanceGate({ isPending: false, isError: true, data: READY })).toBe("ready");
    expect(decideAppInstanceGate({ isPending: false, isError: true, data: NOT_READY })).toBe("not-ready");
  });

  it("reports not-ready for a provisioned instance whose setup is unfinished", () => {
    expect(decideAppInstanceGate({ isPending: false, isError: false, data: NOT_READY })).toBe("not-ready");
  });
});
