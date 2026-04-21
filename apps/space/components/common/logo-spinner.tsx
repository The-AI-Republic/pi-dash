/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { PiSymbol } from "@pi-dash/propel/icons";

export function LogoSpinner() {
  return (
    <div className="flex items-center justify-center">
      <PiSymbol className="h-6 w-auto animate-pulse text-primary sm:h-11" />
    </div>
  );
}
