// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Native window title sync for the top chrome. Desktop mirrors the
// in-app location in the OS window title; the web build has no window
// capability, so the hook is a no-op there.
import * as React from "react";

import { platform } from "../../core/platform/index.js";

export function useWindowTitle(title: string): void {
  const desktop = platform.window;
  React.useEffect(() => {
    desktop?.setTitle(title);
  }, [desktop, title]);
}
