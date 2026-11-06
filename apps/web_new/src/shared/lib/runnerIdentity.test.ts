// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  engineLabel,
  isEngineRunner,
  machineDisplayName,
  runnerDisplayName,
  runnerMachineName,
} from "./runnerIdentity.js";

const machine = { id: "0123456789abcdef", label: "Work laptop", host_label: "rich-mbp" };

describe("runnerIdentity", () => {
  it("prefers the label, then the hostname, then a short id", () => {
    expect(machineDisplayName(machine)).toBe("Work laptop");
    expect(machineDisplayName({ ...machine, label: "" })).toBe("rich-mbp");
    expect(machineDisplayName({ ...machine, label: " ", host_label: "" })).toBe("01234567");
    expect(machineDisplayName({ label: "", host_label: "" })).toBe("Unknown computer");
  });

  it("names engine runners by machine and own-agent runners by their own name", () => {
    const engine = { name: "builtin-x-ws-PROJ", provisioning: "desktop_bundled", dev_machine_detail: machine };
    expect(isEngineRunner(engine)).toBe(true);
    expect(runnerDisplayName(engine)).toBe("Built-in engine · Work laptop");
    expect(runnerDisplayName({ ...engine, provisioning: "manual" })).toBe("builtin-x-ws-PROJ");
  });

  it("falls back to the runner hostname, then the bare product name", () => {
    expect(runnerMachineName({ name: "r", dev_machine_detail: null, host_label: "box" })).toBe("box");
    expect(runnerMachineName({ name: "r", dev_machine_detail: null, host_label: "" })).toBeNull();
    expect(engineLabel(null)).toBe("Built-in engine");
  });
});
