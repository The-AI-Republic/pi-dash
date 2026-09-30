// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { Button } from "./Button";
import { Popover } from "./Popover";
import { Tooltip } from "./Tooltip";

export const Details = (): React.ReactElement => (
  <Popover trigger={<Button>Estimate</Button>} title="Estimate" description="Point the issue">
    <p style={{ margin: 0 }}>Point picker lands here with the feature issue.</p>
  </Popover>
);

export const Hint = (): React.ReactElement => (
  <div style={{ display: "flex", gap: 8 }}>
    <Tooltip content="Create issue">
      <Button variant="primary">New</Button>
    </Tooltip>
  </div>
);
