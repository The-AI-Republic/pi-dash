// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Datetime field helpers: Django renders datetimes in the caller's
// timezone, so every API datetime must accept numeric offsets, not just Z.
import { describe, expect, it } from "vitest";
import { dateTime, nullableDate, nullableDateTime, optionalDateTime } from "./fields.js";
import { Me } from "./users.js";

const ZULU = "2026-10-16T06:32:06.646003Z";
const TOKYO = "2026-10-16T15:32:05.479661+09:00";
const NEW_YORK = "2026-10-16T02:32:05.479661-04:00";

describe("dateTime", () => {
  it.each([ZULU, TOKYO, NEW_YORK])("accepts %s", (value) => {
    expect(dateTime().safeParse(value).success).toBe(true);
  });

  it.each(["not-a-date", "", "2026-10-16", "2026-10-16T15:32:05"])("rejects %s", (value) => {
    expect(dateTime().safeParse(value).success).toBe(false);
  });
});

describe("nullableDate", () => {
  it("accepts null and calendar dates, not datetimes", () => {
    const schema = nullableDate();
    expect(schema.safeParse(null).success).toBe(true);
    expect(schema.safeParse("2026-10-16").success).toBe(true);
    expect(schema.safeParse(TOKYO).success).toBe(false);
    expect(schema.safeParse("not-a-date").success).toBe(false);
  });
});

describe("nullableDateTime", () => {
  it("accepts null and offset datetimes", () => {
    const schema = nullableDateTime();
    expect(schema.safeParse(null).success).toBe(true);
    expect(schema.safeParse(TOKYO).success).toBe(true);
    expect(schema.safeParse("not-a-date").success).toBe(false);
  });
});

describe("optionalDateTime", () => {
  it("accepts undefined and offset datetimes", () => {
    const schema = optionalDateTime();
    expect(schema.safeParse(undefined).success).toBe(true);
    expect(schema.safeParse(TOKYO).success).toBe(true);
    expect(schema.safeParse("not-a-date").success).toBe(false);
  });
});

describe("Me with a non-UTC timezone", () => {
  // Shape of GET /api/users/me/ for a user whose timezone is
  // Asia/Tokyo: the entry gate parses this before every bounce, so a
  // rejection here locks the user on the sign-in card (NEWFRONT-295).
  const tokyoMe = {
    id: "30b62429-0321-4ed3-a162-5b5122c8fa4f",
    email: "parity-oracle@example.com",
    username: "parity_oracle",
    first_name: "Parity",
    last_name: "Oracle",
    display_name: "Parity Oracle",
    avatar: "",
    avatar_url: null,
    cover_image: null,
    cover_image_url: null,
    date_joined: TOKYO,
    user_timezone: "Asia/Tokyo",
    is_active: true,
    is_bot: false,
    is_email_verified: true,
    is_password_autoset: false,
    last_login_medium: "email",
    last_login_time: TOKYO,
  };

  it("parses offset datetimes", () => {
    expect(Me.safeParse(tokyoMe).success).toBe(true);
  });
});
