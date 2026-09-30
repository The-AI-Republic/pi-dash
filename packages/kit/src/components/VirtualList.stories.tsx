// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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
