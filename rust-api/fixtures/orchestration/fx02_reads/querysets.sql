{
  "_trace": "core/querysets.py:19-30 (member_project_issues), :33-52 (user_issues_queryset all/assigned/created)",
  "_method": "CaptureQueriesContext around live queryset evaluation (values_list, sorted) on pidash_524_scratch",
  "executed_sql": {
    "member_project_issues": [
      {
        "sql": "SELECT DISTINCT \"issues\".\"id\", \"issues\".\"created_at\" FROM \"issues\" LEFT OUTER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\") INNER JOIN \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\") INNER JOIN \"project_members\" ON (\"projects\".\"id\" = \"project_members\".\"project_id\") INNER JOIN \"workspaces\" ON (\"issues\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"issues\".\"deleted_at\" IS NULL AND NOT (\"states\".\"group\" = 'triage' AND \"states\".\"group\" IS NOT NULL) AND NOT (\"issues\".\"archived_at\" IS NOT NULL) AND NOT (\"projects\".\"archived_at\" IS NOT NULL) AND NOT (\"issues\".\"is_draft\") AND \"project_members\".\"is_active\" AND \"project_members\".\"member_id\" = '9db9237f42274603a0c70cef71824dad'::uuid AND \"workspaces\".\"slug\" = 'fx2-workspace') ORDER BY \"issues\".\"created_at\" DESC"
      }
    ],
    "user_issues_queryset scope=all": [
      {
        "sql": "SELECT DISTINCT \"issues\".\"id\", \"issues\".\"created_at\" FROM \"issues\" LEFT OUTER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\") INNER JOIN \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\") INNER JOIN \"project_members\" ON (\"projects\".\"id\" = \"project_members\".\"project_id\") INNER JOIN \"workspaces\" ON (\"issues\".\"workspace_id\" = \"workspaces\".\"id\") LEFT OUTER JOIN \"issue_assignees\" ON (\"issues\".\"id\" = \"issue_assignees\".\"issue_id\") LEFT OUTER JOIN \"issue_subscribers\" ON (\"issues\".\"id\" = \"issue_subscribers\".\"issue_id\") WHERE (\"issues\".\"deleted_at\" IS NULL AND NOT (\"states\".\"group\" = 'triage' AND \"states\".\"group\" IS NOT NULL) AND NOT (\"issues\".\"archived_at\" IS NOT NULL) AND NOT (\"projects\".\"archived_at\" IS NOT NULL) AND NOT (\"issues\".\"is_draft\") AND \"project_members\".\"is_active\" AND \"project_members\".\"member_id\" = '9db9237f42274603a0c70cef71824dad'::uuid AND \"workspaces\".\"slug\" = 'fx2-workspace' AND (\"issue_assignees\".\"assignee_id\" = '9db9237f42274603a0c70cef71824dad'::uuid OR \"issues\".\"created_by_id\" = '9db9237f42274603a0c70cef71824dad'::uuid OR \"issue_subscribers\".\"subscriber_id\" = '9db9237f42274603a0c70cef71824dad'::uuid)) ORDER BY \"issues\".\"created_at\" DESC"
      }
    ],
    "user_issues_queryset scope=assigned": [
      {
        "sql": "SELECT DISTINCT \"issues\".\"id\", \"issues\".\"created_at\" FROM \"issues\" LEFT OUTER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\") INNER JOIN \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\") INNER JOIN \"project_members\" ON (\"projects\".\"id\" = \"project_members\".\"project_id\") INNER JOIN \"workspaces\" ON (\"issues\".\"workspace_id\" = \"workspaces\".\"id\") INNER JOIN \"issue_assignees\" ON (\"issues\".\"id\" = \"issue_assignees\".\"issue_id\") WHERE (\"issues\".\"deleted_at\" IS NULL AND NOT (\"states\".\"group\" = 'triage' AND \"states\".\"group\" IS NOT NULL) AND NOT (\"issues\".\"archived_at\" IS NOT NULL) AND NOT (\"projects\".\"archived_at\" IS NOT NULL) AND NOT (\"issues\".\"is_draft\") AND \"project_members\".\"is_active\" AND \"project_members\".\"member_id\" = '9db9237f42274603a0c70cef71824dad'::uuid AND \"workspaces\".\"slug\" = 'fx2-workspace' AND \"issue_assignees\".\"assignee_id\" = '9db9237f42274603a0c70cef71824dad'::uuid) ORDER BY \"issues\".\"created_at\" DESC"
      }
    ],
    "user_issues_queryset scope=created": [
      {
        "sql": "SELECT DISTINCT \"issues\".\"id\", \"issues\".\"created_at\" FROM \"issues\" LEFT OUTER JOIN \"states\" ON (\"issues\".\"state_id\" = \"states\".\"id\") INNER JOIN \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\") INNER JOIN \"project_members\" ON (\"projects\".\"id\" = \"project_members\".\"project_id\") INNER JOIN \"workspaces\" ON (\"issues\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"issues\".\"deleted_at\" IS NULL AND NOT (\"states\".\"group\" = 'triage' AND \"states\".\"group\" IS NOT NULL) AND NOT (\"issues\".\"archived_at\" IS NOT NULL) AND NOT (\"projects\".\"archived_at\" IS NOT NULL) AND NOT (\"issues\".\"is_draft\") AND \"project_members\".\"is_active\" AND \"project_members\".\"member_id\" = '9db9237f42274603a0c70cef71824dad'::uuid AND \"workspaces\".\"slug\" = 'fx2-workspace' AND \"issues\".\"created_by_id\" = '9db9237f42274603a0c70cef71824dad'::uuid) ORDER BY \"issues\".\"created_at\" DESC"
      }
    ]
  }
}
