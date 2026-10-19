// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { Markdown } from "./Markdown";

export const Prose = (): React.ReactElement => (
  <Markdown
    source={
      "# Release notes\n\nA **bold claim** with an [example](https://example.com) link.\n\n- first\n- second\n\n```ts\nconst ready = true;\n```"
    }
  />
);
