{
  "_trace": "orchestration/blockers.py:152-160 (open_blockers_q: Exists forward + Exists stored-reversed over _open targets)",
  "_method": "CaptureQueriesContext around live bulk scan on pidash_524_scratch (default issue_ref='pk')",
  "executed_sql": [
    {
      "sql": "SELECT \"issues\".\"id\" FROM \"issues\" WHERE (\"issues\".\"deleted_at\" IS NULL AND \"issues\".\"id\" IN ('55555555666677778888000000003203'::uuid, '55555555666677778888000000003217'::uuid, '55555555666677778888000000003219'::uuid, '55555555666677778888000000003204'::uuid) AND (EXISTS(SELECT 1 AS \"a\" FROM \"issue_relations\" V0 INNER JOIN \"issues\" V1 ON (V0.\"related_issue_id\" = V1.\"id\") WHERE (V0.\"deleted_at\" IS NULL AND V0.\"deleted_at\" IS NULL AND NOT (V0.\"issue_id\" = (V0.\"related_issue_id\")) AND V0.\"issue_id\" = (\"issues\".\"id\") AND V0.\"related_issue_id\" IN (SELECT U0.\"id\" FROM \"issues\" U0 LEFT OUTER JOIN \"states\" U1 ON (U0.\"state_id\" = U1.\"id\") INNER JOIN \"projects\" U2 ON (U0.\"project_id\" = U2.\"id\") WHERE (U0.\"deleted_at\" IS NULL AND NOT (U1.\"group\" = 'triage' AND U1.\"group\" IS NOT NULL) AND NOT (U0.\"archived_at\" IS NOT NULL) AND NOT (U2.\"archived_at\" IS NOT NULL) AND NOT (U0.\"is_draft\") AND NOT (U1.\"group\" IN ('cancelled', 'completed') AND U1.\"group\" IS NOT NULL))) AND V1.\"workspace_id\" = (V0.\"workspace_id\") AND V0.\"relation_type\" = 'blocked_by') LIMIT 1) OR EXISTS(SELECT 1 AS \"a\" FROM \"issue_relations\" V0 INNER JOIN \"issues\" V2 ON (V0.\"issue_id\" = V2.\"id\") WHERE (V0.\"deleted_at\" IS NULL AND V0.\"deleted_at\" IS NULL AND NOT (V0.\"issue_id\" = (V0.\"related_issue_id\")) AND V0.\"issue_id\" IN (SELECT U0.\"id\" FROM \"issues\" U0 LEFT OUTER JOIN \"states\" U1 ON (U0.\"state_id\" = U1.\"id\") INNER JOIN \"projects\" U2 ON (U0.\"project_id\" = U2.\"id\") WHERE (U0.\"deleted_at\" IS NULL AND NOT (U1.\"group\" = 'triage' AND U1.\"group\" IS NOT NULL) AND NOT (U0.\"archived_at\" IS NOT NULL) AND NOT (U2.\"archived_at\" IS NOT NULL) AND NOT (U0.\"is_draft\") AND NOT (U1.\"group\" IN ('cancelled', 'completed') AND U1.\"group\" IS NOT NULL))) AND V2.\"workspace_id\" = (V0.\"workspace_id\") AND V0.\"related_issue_id\" = (\"issues\".\"id\") AND V0.\"relation_type\" = 'blocking') LIMIT 1))) ORDER BY \"issues\".\"created_at\" DESC"
    }
  ],
  "scan": {
    "candidates": [
      "FX3A-1",
      "FX3A-13",
      "FX3A-15",
      "FX3A-2"
    ],
    "flagged_ids": [
      "55555555-6666-7777-8888-000000003203"
    ],
    "flagged": [
      "FX3A-1"
    ]
  }
}
