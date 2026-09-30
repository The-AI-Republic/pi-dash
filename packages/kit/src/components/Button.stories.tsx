// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { Button } from "./Button";

export const Variants = (): React.ReactElement => (
  <div style={{ display: "flex", gap: 8 }}>
    <Button variant="primary">Primary</Button>
    <Button variant="secondary">Secondary</Button>
    <Button variant="ghost">Ghost</Button>
    <Button variant="danger">Delete</Button>
  </div>
);

export const Sizes = (): React.ReactElement => (
  <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
    <Button size="small">Small</Button>
    <Button size="medium">Medium</Button>
    <Button size="large">Large</Button>
  </div>
);

export const Loading = (): React.ReactElement => (
  <Button variant="primary" loading>
    Saving
  </Button>
);

export const Disabled = (): React.ReactElement => <Button disabled>Unavailable</Button>;
