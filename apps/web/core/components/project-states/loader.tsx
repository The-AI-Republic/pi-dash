/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { Loader } from "@apple-pi-dash/ui";

export function ProjectStateLoader() {
  return (
    <Loader className="space-y-5 md:w-2/3">
      <Loader.Item height="40px" />
      <Loader.Item height="40px" />
      <Loader.Item height="40px" />
      <Loader.Item height="40px" />
    </Loader>
  );
}
