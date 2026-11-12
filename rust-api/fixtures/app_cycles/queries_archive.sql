-- queries_archive.sql
-- CycleArchiveUnarchiveEndpoint.get_queryset record: SQL shape + result rows (rows in queries_archive.rows.json).
-- Source: app/views/cycle/archive.py:41-270. Ported-from: 01a93e17216faea7bfc156b0f864cbbe420d1c52.
--
-- R1 base filters (:114-123): SAME membership/project scoping as base queryset
--   EXCEPT archived_at__isnull=False (:117) — archived cycles only (vs =True/unfiltered in base).
-- R2 counts: total/completed/cancelled identical to base (:137-171) PLUS
--   started_issues (:172-183), unstarted_issues (:184-195), backlog_issues (:196-207)
--   with the same archived/is_draft/deleted guards. No deleted_at guard on the
--   bridge for total (:141-146 omits issue_cycle__deleted_at__isnull — BUG-PORT:
--   base total (:118-124) HAS it, archive total does NOT; PORT the asymmetry).
-- R3 status Case uses timezone.now() DIRECTLY (:209-223), not the project-tz
--   round-trip of base.py:78-88 — same instants, PORT the value.
-- R4 assignee_ids Coalesce-ArrayAgg WITHOUT the issue_assignee deleted_at filter
--   (:224-233 vs base :168-178 which has it) — BUG-PORT the asymmetry.
-- R5 per-group estimate subqueries (:49-113 -> :234-266): for each of
--   backlog/unstarted/started/cancelled/completed/total, a correlated Subquery over
--   Issue.issue_objects with estimate_point__estimate__type='points',
--   state__group=<group> (total: no group filter), issue_cycle__cycle_id=OuterRef('pk'),
--   issue_cycle__deleted_at IS NULL, SUM(CAST(estimate_point.value AS FLOAT)),
--   wrapped Coalesce(<subquery>, 0.0). No archived/is_draft guards on these
--   subqueries — PORT as observed.
-- R6 tail: .order_by('-is_favorite', 'name') (:267) + .distinct() (:268).
-- R7 get() (:271-304, pk=None): .values() incl. archived_at, ordered
--   ('-is_favorite', '-created_at') (:303) — same dead-first-ordering pattern as base list.
-- R8 detail get (:305-584): sub_issues annotation (:310-320, same shape as base
--   retrieve) + completed/total estimate points + estimate_distribution (assignees/
--   labels/completed/pending estimate Sums, :367-459, only when project estimate
--   type is points :358-363) + distribution (issue counts by assignee/label,
--   :470-573) + burndown_plot completion charts when start+end set (:461-468, :575-582).
-- R9 archive post (:586-604): requires cycle.end_date >= timezone.now(), else 400
--   {"error": "Only completed cycles can be archived"} (:590-594).
--   -- BUG-PORT: end_date NULL raises TypeError (None >= datetime) -> 500, not 400.
--   On success: archived_at=now, save, delete matching UserFavorites, 200
--   {"archived_at": str(cycle.archived_at)} (:596-604).
-- R10 unarchive delete (:606-611): archived_at=None, save, 204 (no date guard).
--
-- Representative SQL additions over queries_base.sql (placeholders :slug, :project_id, :user):
SELECT DISTINCT cycles.*,
  -- ... same is_favorite/total/completed/cancelled/status/assignee_ids as base ...
  COUNT(DISTINCT issue.id) FILTER (WHERE issue.state_group = 'started'
    AND issue.archived_at IS NULL AND issue.is_draft = FALSE
    AND issue.deleted_at IS NULL) AS started_issues,
  COUNT(DISTINCT issue.id) FILTER (WHERE issue.state_group = 'unstarted'
    AND issue.archived_at IS NULL AND issue.is_draft = FALSE
    AND issue.deleted_at IS NULL) AS unstarted_issues,
  COUNT(DISTINCT issue.id) FILTER (WHERE issue.state_group = 'backlog'
    AND issue.archived_at IS NULL AND issue.is_draft = FALSE
    AND issue.deleted_at IS NULL) AS backlog_issues,
  COALESCE((SELECT SUM(CAST(estimate_point.value AS FLOAT)) FROM issues ie
    JOIN estimates e ON e.id = ie.estimate_point_id
    JOIN cycle_issues ci2 ON ci2.issue_id = ie.id AND ci2.deleted_at IS NULL
    WHERE ci2.cycle_id = cycles.id AND e.type = 'points'
      AND ie.state_group = 'backlog'), 0.0) AS backlog_estimate_points,
  -- ... same shape for unstarted/started/cancelled/completed (add state_group filter),
  -- total_estimate_points (no state_group filter) ...
  COALESCE((SELECT SUM(CAST(estimate_point.value AS FLOAT)) FROM issues ie
    JOIN estimates e ON e.id = ie.estimate_point_id
    JOIN cycle_issues ci2 ON ci2.issue_id = ie.id AND ci2.deleted_at IS NULL
    WHERE ci2.cycle_id = cycles.id AND e.type = 'points'), 0.0) AS total_estimate_points
FROM cycles
-- ... same joins/where as base EXCEPT cycles.archived_at IS NOT NULL ...
ORDER BY is_favorite DESC, cycles.name ASC;
-- EFFECTIVE archived-list order: ORDER BY is_favorite DESC, cycles.created_at DESC (:303).
