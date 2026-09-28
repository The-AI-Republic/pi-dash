{
  "_trace": "bgtasks/agent_ticker.py:57-69 (enabled + next_run_at<=now + pending/INFINITE/cap OR + ORDER BY next_run_at)",
  "_method": "ran scan_due_tickers live on scratch DB pidash_ticker_fx (frozen T0); SQL = executed statements via CaptureQueriesContext",
  "fanout_count": 5,
  "fanout_order": [
    "T1-due-under-cap",
    "T6-granted",
    "T7-waited",
    "T5-spent-pool-pending",
    "T8-infinite"
  ],
  "executed_sql": [
    {
      "sql": "SELECT \"issue_agent_ticker\".\"id\" FROM \"issue_agent_ticker\" INNER JOIN \"issues\" ON (\"issue_agent_ticker\".\"issue_id\" = \"issues\".\"id\") INNER JOIN \"projects\" ON (\"issues\".\"project_id\" = \"projects\".\"id\") WHERE (\"issue_agent_ticker\".\"deleted_at\" IS NULL AND \"issue_agent_ticker\".\"enabled\" AND \"issue_agent_ticker\".\"next_run_at\" <= '2026-09-28 12:00:00+00:00'::timestamptz AND (\"issue_agent_ticker\".\"pending_entry\" OR \"projects\".\"agent_default_max_ticks\" =  -1 OR \"issue_agent_ticker\".\"used\" < ((\"projects\".\"agent_default_max_ticks\" + \"issue_agent_ticker\".\"granted\") + \"issue_agent_ticker\".\"waited\"))) ORDER BY \"issue_agent_ticker\".\"next_run_at\" ASC",
      "params": null
    }
  ]
}
