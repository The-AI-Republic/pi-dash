// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-115): @-mention autocomplete in the comment
// composer. Typing the trigger offers the matching workspace members with
// avatars under section headers, and the list narrows while typing.
// Row: CMT-019.
import { test, expect } from "../fixtures";
import {
  requireMentionMember,
  serverUsableSeedIssue,
  serverUserMentionSuggestions,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

const ROWS = ["CMT-019"];

test(
  specTitle(ROWS, "mention autocomplete offers members and narrows while typing"),
  { tag: specTags(ROWS) },
  async ({ driver, seed }) => {
    const member = requireMentionMember(seed);

    await test.step("sign in through the UI", async () => {
      await driver.mentionsEnsureSignedIn(seed.email, seed.password);
    });

    let issueId = "";
    await test.step("open a seeded issue", async () => {
      const session = await signInSession(seed.email, seed.password);
      // Select by seeded name (probe issues from sibling runs make position
      // unstable) and skip issues triaged into intake, which redirect away
      // from the comment composer.
      const target = await serverUsableSeedIssue(seed.workspaceSlug, seed.projectId, seed.issueNames, session);
      issueId = target.id;
      await driver.mentionsOpenIssueDetail(seed.workspaceSlug, seed.projectId, issueId);
    });

    await test.step("a broad query offers both seed members with avatars", async () => {
      const names = await driver.mentionsSuggestionsFor("Pari");
      expect(names).toEqual(expect.arrayContaining([member.displayName, "Parity Oracle"]));
      // The trigger offers up to a handful of matches, never an open-ended list.
      expect(names.length).toBeLessThanOrEqual(5);
      expect(await driver.mentionsSuggestionsHaveAvatars()).toBe(true);
      expect(await driver.mentionsSuggestionSections()).toContain("Users");
    });

    await test.step("typing more narrows the list to the match", async () => {
      const names = await driver.mentionsSuggestionsFor("Parity M");
      expect(names).toEqual([member.displayName]);
    });

    await test.step("the server narrows the same way the screen does", async () => {
      const session = await signInSession(seed.email, seed.password);
      const broad = await serverUserMentionSuggestions(seed.workspaceSlug, seed.projectId, session, "Pari");
      expect(broad.map((s) => s.displayName)).toEqual(expect.arrayContaining([member.displayName, "Parity Oracle"]));
      expect(broad.length).toBeLessThanOrEqual(5);
      const narrow = await serverUserMentionSuggestions(seed.workspaceSlug, seed.projectId, session, "Parity M");
      expect(narrow.map((s) => s.displayName)).toEqual([member.displayName]);
      expect(narrow[0]!.id).toBe(member.id);
    });
  }
);
