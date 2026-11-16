/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { range } from "lodash-es";

export function ProjectsLoader() {
  return (
    <div className="h-full w-full animate-pulse overflow-y-auto p-8">
      <div className="flex flex-col divide-y divide-subtle overflow-hidden rounded-lg border border-subtle bg-layer-2">
        {range(6).map((i) => (
          <div key={i} className="flex w-full items-center gap-3 px-4 py-3">
            <span className="h-9 w-9 flex-shrink-0 rounded-sm bg-layer-1" />
            <div className="flex min-w-0 flex-grow flex-col justify-center gap-1.5">
              <span className="h-4 w-40 rounded-sm bg-layer-1" />
              <span className="h-3 w-24 rounded-sm bg-layer-1" />
            </div>
            <div className="flex flex-shrink-0 items-center gap-2">
              <span className="h-5 w-16 rounded-full bg-layer-1" />
              <span className="h-6 w-6 rounded-sm bg-layer-1" />
              <span className="h-6 w-6 rounded-sm bg-layer-1" />
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
