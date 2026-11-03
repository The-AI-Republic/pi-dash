/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { Scissors } from "lucide-react";

/**
 * Inline notice under a reply the model did not finish: it stopped at its
 * output limit (`finish_reason=length`), which the backend records as
 * `payload.truncated` on the message. Without it the reply just ends
 * mid-sentence and reads as if the assistant had nothing more to say.
 */
export function ReplyCutOffNotice() {
  return (
    <div className="flex items-start gap-2 rounded-md border border-subtle bg-surface-1 px-3 py-2 text-12 text-secondary">
      <Scissors className="mt-0.5 size-3.5 shrink-0" />
      <span>
        This reply was cut off because it reached the model&apos;s output limit. Ask the assistant to continue.
      </span>
    </div>
  );
}
