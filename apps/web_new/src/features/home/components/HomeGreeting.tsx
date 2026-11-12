// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Home greeting (SHELL-004, SHELL-005; written from parity/specs/shell.md
// §2). A time-of-day salutation naming the user, with a fixed-shape date
// line and a live clock that advances on a client ticker without reload.
// The zone comes from the user record and the language from the profile;
// both degrade through the greeting helpers, never blank.

import * as React from "react";

import { useMe, useProfile } from "../../../core/session/index.js";
import {
  formatGreetingHeading,
  formatHomeDateLine,
  hourInTimeZone,
  selectDayPart,
  selectSalutation,
} from "../lib/greeting.js";

/** The current instant, refreshed on a client ticker. */
export function useNow(intervalMs = 1000): Date {
  const [now, setNow] = React.useState(() => new Date());
  React.useEffect(() => {
    const timer = globalThis.setInterval(() => {
      setNow(new Date());
    }, intervalMs);
    return () => {
      globalThis.clearInterval(timer);
    };
  }, [intervalMs]);
  return now;
}

export function HomeGreeting(): React.ReactElement | null {
  const me = useMe();
  const profile = useProfile();
  const now = useNow();
  if (me.data === undefined) return null;
  const timeZone = me.data.user_timezone;
  const dayPart = selectDayPart(hourInTimeZone(now, timeZone));
  const heading = formatGreetingHeading(selectSalutation(profile.data?.language, dayPart), me.data.first_name);
  return (
    <div className="flex flex-col items-center gap-(--space-1) text-center">
      <h1 id="home-greeting" className="text-h1 text-(--text)">
        {heading}
      </h1>
      <p id="home-date-line" className="text-body text-(--text-muted)">
        {formatHomeDateLine(now, timeZone)}
      </p>
    </div>
  );
}
