// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
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
