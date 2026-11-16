// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// External help destinations (SHELL-089); all leave the app. Eager-only
// leaf: eager shell components import these from here, never from the
// palette catalog (entries/account.js mirrors the same values for its
// lazy consumers; account.test.ts pins the two spellings together). A
// module imported from both a route's eager closure and its lazy
// chunks collapses that route's TanStack component split and trips the
// size gate (NEWFRONT-287), so this file must stay out of entries/ and
// every lazy chunk.
export const DOCS_URL = "https://github.com/The-AI-Republic/pi-dash#readme";
export const FORUM_URL = "https://github.com/The-AI-Republic/pi-dash/discussions";
export const REPORT_BUG_URL = "https://github.com/The-AI-Republic/pi-dash/issues/new/choose";
