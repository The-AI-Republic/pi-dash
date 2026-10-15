// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { filterTimezoneOptions, selectTimezoneOptions } from "./timezone.js";

const ENTRIES = [
  { utc_offset: "+09:00", gmt_offset: "GMT+9", value: "Asia/Tokyo", label: "Tokyo" },
  { utc_offset: "+11:00", gmt_offset: "GMT+11", value: "Australia/Sydney", label: "Sydney" },
  { utc_offset: "+09:00", gmt_offset: "GMT+9", value: "Asia/Tokyo", label: "Tokyo" },
  { utc_offset: "+05:30", gmt_offset: "GMT+5:30", value: "Asia/Kolkata", label: "Kolkata" },
];

describe("timezone options", () => {
  it("merges duplicate values, sorts by offset label and appends UTC", () => {
    const options = selectTimezoneOptions(ENTRIES);
    expect(options.map((option) => option.value)).toEqual(["Asia/Kolkata", "Asia/Tokyo", "Australia/Sydney", "UTC"]);
    expect(options[1]).toMatchObject({ label: "Tokyo", detail: "+09:00 Tokyo" });
    expect(options[3]).toEqual({ value: "UTC", label: "UTC", detail: "UTC", query: "utc, coordinated universal time" });
  });

  it("joins distinct labels that share a value", () => {
    const options = selectTimezoneOptions([
      { utc_offset: "+01:00", gmt_offset: "GMT+1", value: "Europe/Paris", label: "Paris" },
      { utc_offset: "+01:00", gmt_offset: "GMT+1", value: "Europe/Paris", label: "France" },
    ]);
    expect(options[0]).toMatchObject({ value: "Europe/Paris", label: "Paris, France" });
  });

  it("matches cities, regions and offsets case-insensitively", () => {
    const options = selectTimezoneOptions(ENTRIES);
    expect(filterTimezoneOptions(options, "")).toHaveLength(4);
    expect(filterTimezoneOptions(options, "tok").map((option) => option.value)).toEqual(["Asia/Tokyo"]);
    expect(filterTimezoneOptions(options, "SYDNEY").map((option) => option.value)).toEqual(["Australia/Sydney"]);
    expect(filterTimezoneOptions(options, "gmt+9").map((option) => option.value)).toEqual(["Asia/Tokyo"]);
    expect(filterTimezoneOptions(options, "+05:30").map((option) => option.value)).toEqual(["Asia/Kolkata"]);
    expect(filterTimezoneOptions(options, "coordinated").map((option) => option.value)).toEqual(["UTC"]);
    expect(filterTimezoneOptions(options, "nowhere")).toHaveLength(0);
  });
});
