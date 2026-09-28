{
  "_trace": "bgtasks/scheduler.py:115-128 (enabled + scheduler is_enabled + both deleted_at NULL + project deleted_at NULL + next_run_at<=now OR NULL + ORDER BY)",
  "_method": "ran scan_due_bindings live on pidash_ticker_fx (frozen T0); SQL = executed statements via CaptureQueriesContext",
  "fanout_count": 2,
  "fanout_order": [
    "B6-past-due",
    "B1-null-due"
  ],
  "executed_sql": [
    {
      "sql": "SELECT \"scheduler_bindings\".\"id\" FROM \"scheduler_bindings\" LEFT OUTER JOIN \"projects\" ON (\"scheduler_bindings\".\"project_id\" = \"projects\".\"id\") INNER JOIN \"schedulers\" ON (\"scheduler_bindings\".\"scheduler_id\" = \"schedulers\".\"id\") WHERE (\"scheduler_bindings\".\"deleted_at\" IS NULL AND \"scheduler_bindings\".\"enabled\" AND \"projects\".\"deleted_at\" IS NULL AND \"schedulers\".\"deleted_at\" IS NULL AND \"schedulers\".\"is_enabled\" AND (\"scheduler_bindings\".\"next_run_at\" <= '2026-09-28 12:00:00+00:00'::timestamptz OR \"scheduler_bindings\".\"next_run_at\" IS NULL)) ORDER BY \"scheduler_bindings\".\"next_run_at\" ASC",
      "params": null
    }
  ]
}
