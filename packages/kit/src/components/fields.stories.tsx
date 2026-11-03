// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
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
