/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { Loader } from "@apple-pi-dash/ui";

export function FilterItemLoader() {
  return (
    <Loader>
      <Loader.Item height="28px" width="180px" />
    </Loader>
  );
}
