// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { Input } from "./Input";
import { Textarea } from "./Textarea";

export const TextField = (): React.ReactElement => (
  <div style={{ display: "flex", flexDirection: "column", gap: 16, maxWidth: 320 }}>
    <Input label="Title" placeholder="Name it" hint="Shown in the issue list" />
    <Input label="Title" defaultValue="Needs a name" error="Required" />
    <Input label="Title" defaultValue="Locked" disabled />
  </div>
);

export const Multiline = (): React.ReactElement => (
  <div style={{ display: "flex", flexDirection: "column", gap: 16, maxWidth: 320 }}>
    <Textarea label="Description" placeholder="Write it down" hint="Markdown is supported" />
    <Textarea label="Description" defaultValue="Too much" error="Keep it under a paragraph" />
  </div>
);
