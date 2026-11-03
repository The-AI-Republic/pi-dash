// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { VirtualList } from "./VirtualList";

const rows = Array.from({ length: 2000 }, (_, index) => `Issue ${index + 1}`);

export const LongList = (): React.ReactElement => (
  <div style={{ height: 280, border: "1px solid var(--border)", borderRadius: 6 }}>
    <VirtualList
      label="Issues"
      items={rows}
      estimateSize={28}
      getKey={(item) => item}
      renderRow={(item) => (
        <div
          style={{
            height: 28,
            display: "flex",
            alignItems: "center",
            padding: "0 12px",
            borderBottom: "1px solid var(--border)",
          }}
        >
          {item}
        </div>
      )}
      className="h-full"
    />
  </div>
);
