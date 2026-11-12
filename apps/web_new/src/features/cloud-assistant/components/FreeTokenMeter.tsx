// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Free-lane usage meter (NEWPIDASH-149). One line in the AI Assistant
// tab: how much of today's free allowance is used and when it resets.
// The read is best-effort by design — while it loads, and when the
// quota endpoint errors, the meter renders nothing, so a missing or
// failing quota never blocks the page or a turn.

import * as React from "react";

import { useFreeTokenUsage } from "../api/queries.js";
import { freeTokenMeterText } from "../lib/quota.js";

export function FreeTokenMeter(): React.ReactElement | null {
  const { data } = useFreeTokenUsage();
  if (data === undefined) return null;
  return (
    <p className="text-body text-(--text-muted)" data-testid="free-token-meter">
      {freeTokenMeterText(data.llm.freeTokensUsed, data.llm.freeTokensLimit, data.resetsAt)}
    </p>
  );
}
