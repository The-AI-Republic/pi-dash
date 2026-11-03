// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  cloudEntryReturnTo,
  decideCloudEntry,
  toCloudEntryProbe,
  toCloudEntrySession,
  type CloudEntryProbeInput,
  type CloudEntryProbeState,
  type CloudEntrySessionInput,
} from "./cloudEntry.js";

describe("toCloudEntrySession", () => {
  it("reads each settled verdict through", () => {
    const settled = (kind: "admin" | "anonymous" | "forbidden"): CloudEntrySessionInput => ({
      isPending: false,
      isError: false,
      data: kind === "admin" ? { kind, admin: {} as never } : { kind },
    });
    expect(toCloudEntrySession(settled("admin"))).toBe("admin");
    expect(toCloudEntrySession(settled("anonymous"))).toBe("anonymous");
    expect(toCloudEntrySession(settled("forbidden"))).toBe("forbidden");
  });

  it("maps a failed check to an outage, never to anonymous", () => {
    expect(toCloudEntrySession({ isPending: false, isError: true, data: undefined })).toBe("error");
  });

  it("stays pending while the check is unresolved or unset", () => {
    expect(toCloudEntrySession({ isPending: true, isError: false, data: undefined })).toBe("pending");
    expect(toCloudEntrySession({ isPending: false, isError: false, data: undefined })).toBe("pending");
  });
});

describe("toCloudEntryProbe", () => {
  it("reads each settled verdict through", () => {
    const settled = (
      kind: CloudEntryProbeState & ("authenticated" | "anonymous" | "inconclusive")
    ): CloudEntryProbeInput => ({
      isPending: false,
      isError: false,
      data: kind === "authenticated" ? { kind, user: {} as never } : { kind },
    });
    expect(toCloudEntryProbe(settled("authenticated"))).toBe("authenticated");
    expect(toCloudEntryProbe(settled("anonymous"))).toBe("anonymous");
    expect(toCloudEntryProbe(settled("inconclusive"))).toBe("inconclusive");
  });

  it("maps a failed probe read to inconclusive, never to a guess", () => {
    expect(toCloudEntryProbe({ isPending: false, isError: true, data: undefined })).toBe("inconclusive");
  });

  it("stays pending while the probe is unresolved or unset", () => {
    expect(toCloudEntryProbe({ isPending: true, isError: false, data: undefined })).toBe("pending");
    expect(toCloudEntryProbe({ isPending: false, isError: false, data: undefined })).toBe("pending");
  });
});

describe("decideCloudEntry", () => {
  const probes: CloudEntryProbeState[] = ["pending", "authenticated", "anonymous", "inconclusive"];

  it("holds the spinner while the check is unresolved or the guard redirects an admin", () => {
    for (const probe of probes) {
      expect(decideCloudEntry("pending", probe)).toBe("spinner");
      expect(decideCloudEntry("admin", probe)).toBe("spinner");
    }
  });

  it("bounces an anonymous read without consulting the probe", () => {
    for (const probe of probes) {
      expect(decideCloudEntry("anonymous", probe)).toBe("bounce");
    }
  });

  it("retries a failed check, never denying or bouncing on an outage", () => {
    for (const probe of probes) {
      expect(decideCloudEntry("error", probe)).toBe("retry");
    }
  });

  it("denies a forbidden read only on an authenticated probe", () => {
    expect(decideCloudEntry("forbidden", "authenticated")).toBe("denied");
  });

  it("bounces a forbidden read on an anonymous probe", () => {
    expect(decideCloudEntry("forbidden", "anonymous")).toBe("bounce");
  });

  it("holds the spinner while the forbidden probe is unresolved", () => {
    expect(decideCloudEntry("forbidden", "pending")).toBe("spinner");
  });

  it("retries a forbidden read on an inconclusive probe, never guessing anonymous", () => {
    expect(decideCloudEntry("forbidden", "inconclusive")).toBe("retry");
  });
});

describe("cloudEntryReturnTo", () => {
  it("keeps the path plus query as a relative reference", () => {
    expect(cloudEntryReturnTo("/god-mode/", "")).toBe("/god-mode/");
    expect(cloudEntryReturnTo("/god-mode/loop/3/", "?tab=targets")).toBe("/god-mode/loop/3/?tab=targets");
  });
});
