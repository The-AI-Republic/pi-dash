// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Boot locale stamp for the space subtree (SPC-065). Heals an
// unsupported stored tag to English once per load and stamps the
// document language; renders nothing.
import * as React from "react";

import { healSpaceLocale } from "../lib/spaceLocale.js";

export function SpaceLocaleGate(): React.ReactNode {
  React.useEffect(() => {
    healSpaceLocale();
  }, []);
  return null;
}
