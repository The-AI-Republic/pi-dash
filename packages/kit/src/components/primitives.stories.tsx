// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { Avatar } from "./Avatar";
import { Badge } from "./Badge";
import { Kbd } from "./Kbd";
import { Skeleton } from "./Skeleton";
import { Spinner } from "./Spinner";

export const Loaders = (): React.ReactElement => (
  <div style={{ display: "flex", gap: 16, alignItems: "center" }}>
    <Spinner size="small" />
    <Spinner size="medium" />
    <Spinner size="large" />
  </div>
);

export const Placeholders = (): React.ReactElement => (
  <div style={{ display: "flex", flexDirection: "column", gap: 8, width: 240 }}>
    <Skeleton height="16px" width="60%" />
    <Skeleton height="12px" />
    <Skeleton height="12px" width="80%" />
  </div>
);

export const Shortcuts = (): React.ReactElement => (
  <div style={{ display: "flex", gap: 12, alignItems: "center" }}>
    <Kbd keys={["mod", "K"]} />
    <Kbd keys={["esc"]} />
  </div>
);

export const Statuses = (): React.ReactElement => (
  <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
    <Badge>3</Badge>
    <Badge tone="accent" dot>
      In review
    </Badge>
    <Badge tone="success" dot>
      Done
    </Badge>
    <Badge tone="warning" dot>
      At risk
    </Badge>
    <Badge tone="danger" dot>
      Blocked
    </Badge>
  </div>
);

export const People = (): React.ReactElement => (
  <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
    <Avatar name="Ada Lovelace" size="small" />
    <Avatar name="Ada Lovelace" size="medium" />
    <Avatar name="Grace Hopper" size="large" />
  </div>
);
