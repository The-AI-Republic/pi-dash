-- queries/issue_retrieve.sql
-- IssueRetrievePublicEndpoint.get views/issue.py:597-773. AllowAny (:595).
-- Board lookup DeployBoard.objects.get(anchor=anchor) with NO entity_name
-- scoping (:598) — PORT (B16). Direct Response(200) (:773), no serializer.
-- Ported-from: 01a93e17216faea7bfc156b0f864cbbe420d1c52.

-- R1 queryset views/issue.py:600-771:
-- Issue.issue_objects.filter(pk, workspace__slug, project_id)
--   .select_related(workspace, project, state, parent) (:606)
--   .prefetch_related(assignees, labels, issue_module__module) (:607)
--   .annotate(cycle_id=Subquery(...[:1])) (:608-612)
--   .annotate(label_ids=Coalesce(ArrayAgg("labels__id", distinct,
--     filter=~Q(labels__id__isnull) & Q(label_issue__deleted_at__isnull)),
--     Value([], ArrayField(UUID)))) (:613-621)
--   .annotate(assignee_ids=Coalesce(ArrayAgg("assignees__id", distinct,
--     filter=~Q(assignees__id__isnull)
--       & Q(assignees__member_project__is_active=True)
--       & Q(issue_assignee__deleted_at__isnull)), Value([]))) (:622-633)
--     NOTE: assignees require an ACTIVE ProjectMember row — PORT.
--   .annotate(module_ids=Coalesce(ArrayAgg("issue_module__module_id", distinct,
--     filter=~Q(module_id__isnull) & Q(module__archived_at__isnull)
--       & Q(issue_module__deleted_at__isnull)), Value([]))) (:634-643)
--     NOTE: archived modules excluded here (stricter than grouper default
--     annotation which only guards null) — PORT.
--   .prefetch_related(Prefetch(issue_reactions,
--     IssueReaction.objects.select_related("issue","actor"))) (:645-650)
--   .prefetch_related(Prefetch(votes, IssueVote.objects.select_related("actor"))) (:651)
--   .annotate(vote_items=ArrayAgg(Case(When(votes NOT NULL AND not deleted,
--     JSONObject(vote, actor_details{...avatar_url CASE...})), default None),
--     filter=Case(...then True...), distinct)) (:652-698)
--   .annotate(reaction_items=ArrayAgg(... on issue_reactions__* ...)) (:699-744)
--   BUG-PORT (:713,:716,:722): reaction_items avatar_url inner Whens ref
--   votes__actor__avatar_asset / votes__actor__avatar instead of
--   issue_reactions__actor__* (copy-paste from vote_items :665-677).
--   .values("id","name","state_id","sort_order","description_json",
--     "description_html","description_stripped","description_binary",
--     "module_ids","label_ids","assignee_ids","estimate_point","priority",
--     "start_date","target_date","sequence_id","project_id","parent_id",
--     "cycle_id","created_by","state__group","vote_items","reaction_items")
--     — 23 keys verbatim (:746-770); .first() (:771).
SELECT "issues"."id", "issues"."name", "issues"."state_id", "issues"."sort_order",
  "issues"."description_json", "issues"."description_html",
  "issues"."description_stripped", "issues"."description_binary",
  COALESCE(ARRAY_AGG(DISTINCT "labels"."id")
    FILTER (WHERE NOT ("labels"."id" IS NULL)
      AND "label_through"."deleted_at" IS NULL), '{}') AS "label_ids",
  COALESCE(ARRAY_AGG(DISTINCT "assignees"."id")
    FILTER (WHERE NOT ("assignees"."id" IS NULL)
      AND "project_members"."is_active" = true
      AND "assignee_through"."deleted_at" IS NULL), '{}') AS "assignee_ids",
  COALESCE(ARRAY_AGG(DISTINCT "modules"."id")
    FILTER (WHERE NOT ("modules"."id" IS NULL)
      AND "modules"."archived_at" IS NULL
      AND "module_through"."deleted_at" IS NULL), '{}') AS "module_ids",
  "issues"."estimate_point_id" AS "estimate_point", "issues"."priority",
  "issues"."start_date", "issues"."target_date", "issues"."sequence_id",
  "issues"."project_id", "issues"."parent_id",
  (SELECT U0."cycle_id" FROM "cycle_issues" U0 WHERE (U0."deleted_at" IS NULL
    AND U0."issue_id" = ("issues"."id")) LIMIT 1) AS "cycle_id",
  "issues"."created_by_id" AS "created_by", "states"."group" AS "state__group",
  ARRAY_AGG(DISTINCT (CASE WHEN ("votes"."id" IS NOT NULL
    AND "votes"."deleted_at" IS NULL) THEN JSONB_BUILD_OBJECT('vote', "votes"."vote",
    'actor_details', JSONB_BUILD_OBJECT('id', "vote_actor"."id",
      'first_name', "vote_actor"."first_name", 'last_name', "vote_actor"."last_name",
      'avatar', "vote_actor"."avatar",
      'avatar_url', (CASE WHEN ("vote_actor"."avatar_asset_id" IS NOT NULL)
        THEN CONCAT('/api/assets/v2/static/', "vote_actor"."avatar_asset_id", '/')
        WHEN ("vote_actor"."avatar_asset_id" IS NULL) THEN "vote_actor"."avatar"
        ELSE NULL END),
      'display_name', "vote_actor"."display_name") ) ELSE NULL END))
    FILTER (WHERE CASE WHEN ("votes"."id" IS NOT NULL
      AND "votes"."deleted_at" IS NULL) THEN true ELSE false END) AS "vote_items",
  ARRAY_AGG(DISTINCT (CASE WHEN ("issue_reactions"."id" IS NOT NULL
    AND "issue_reactions"."deleted_at" IS NULL) THEN JSONB_BUILD_OBJECT(
    'reaction', "issue_reactions"."reaction",
    'actor_details', JSONB_BUILD_OBJECT('id', "reaction_actor"."id",
      'first_name', "reaction_actor"."first_name",
      'last_name', "reaction_actor"."last_name",
      'avatar', "reaction_actor"."avatar",
      -- BUG-PORT views/issue.py:713,716,722: avatar branches read the VOTE
      -- actor traversal (votes__actor__*) instead of issue_reactions__actor__*,
      -- rendered through the vote_actor join reused from vote_items.
      'avatar_url', (CASE WHEN ("vote_actor"."avatar_asset_id" IS NOT NULL)
        THEN CONCAT('/api/assets/v2/static/', "vote_actor"."avatar_asset_id", '/')
        WHEN ("vote_actor"."avatar_asset_id" IS NULL) THEN "vote_actor"."avatar"
        ELSE NULL END),
      'display_name', "reaction_actor"."display_name") ) ELSE NULL END))
    FILTER (WHERE CASE WHEN ("issue_reactions"."id" IS NOT NULL
      AND "issue_reactions"."deleted_at" IS NULL) THEN true ELSE false END) AS "reaction_items"
  FROM "issues" ... (joins per select_related/prefetch paths above)
  WHERE ("issues"."deleted_at" IS NULL AND triage/archived/draft exclusions
    AND "issues"."id" = %(issue_id)s
    AND "workspaces"."slug" = %(slug)s AND "issues"."project_id" = %(project_id)s)
  GROUP BY "issues"."id", "states"."group" LIMIT 1;
