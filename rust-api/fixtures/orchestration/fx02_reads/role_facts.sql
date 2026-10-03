{
  "_trace": "core/permissions.py:28-34 (is_workspace_member), :37-45 (workspace_role), :48-58 (workspace_role_by_slug), :61-70 (admin/member decisions via workspace_role), :73-119 (check_project_role: allowed EXISTS + bypass EXISTS x2)",
  "_method": "CaptureQueriesContext around live permission-fact calls on pidash_524_scratch (SELECTs only; decisions computed in Python by the F-06 kernel contract)",
  "executed_sql": {
    "is_workspace_member": [
      {
        "sql": "SELECT 1 AS \"a\" FROM \"workspace_members\" WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = '615761be2c6c4a75abc9bed145879e9f'::uuid AND \"workspace_members\".\"workspace_id\" = '11111111222233334444000000001001'::uuid) LIMIT 1"
      },
      {
        "sql": "SELECT 1 AS \"a\" FROM \"workspace_members\" WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = '9db9237f42274603a0c70cef71824dad'::uuid AND \"workspace_members\".\"workspace_id\" = '11111111222233334444000000001001'::uuid) LIMIT 1"
      },
      {
        "sql": "SELECT 1 AS \"a\" FROM \"workspace_members\" WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = 'c24bd2ec9378435d847682dcd940b609'::uuid AND \"workspace_members\".\"workspace_id\" = '11111111222233334444000000001001'::uuid) LIMIT 1"
      },
      {
        "sql": "SELECT 1 AS \"a\" FROM \"workspace_members\" WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = '7233fbaabf55448ea615d28408d24335'::uuid AND \"workspace_members\".\"workspace_id\" = '11111111222233334444000000001001'::uuid) LIMIT 1"
      },
      {
        "sql": "SELECT 1 AS \"a\" FROM \"workspace_members\" WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = 'b942920ca7624f1889e60ed6f8702c7f'::uuid AND \"workspace_members\".\"workspace_id\" = '11111111222233334444000000001001'::uuid) LIMIT 1"
      }
    ],
    "workspace_role": [
      {
        "sql": "SELECT \"workspace_members\".\"role\" FROM \"workspace_members\" WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = '615761be2c6c4a75abc9bed145879e9f'::uuid AND \"workspace_members\".\"workspace_id\" = '11111111222233334444000000001001'::uuid) ORDER BY \"workspace_members\".\"created_at\" DESC LIMIT 1"
      },
      {
        "sql": "SELECT \"workspace_members\".\"role\" FROM \"workspace_members\" WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = '9db9237f42274603a0c70cef71824dad'::uuid AND \"workspace_members\".\"workspace_id\" = '11111111222233334444000000001001'::uuid) ORDER BY \"workspace_members\".\"created_at\" DESC LIMIT 1"
      },
      {
        "sql": "SELECT \"workspace_members\".\"role\" FROM \"workspace_members\" WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = 'c24bd2ec9378435d847682dcd940b609'::uuid AND \"workspace_members\".\"workspace_id\" = '11111111222233334444000000001001'::uuid) ORDER BY \"workspace_members\".\"created_at\" DESC LIMIT 1"
      },
      {
        "sql": "SELECT \"workspace_members\".\"role\" FROM \"workspace_members\" WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = '7233fbaabf55448ea615d28408d24335'::uuid AND \"workspace_members\".\"workspace_id\" = '11111111222233334444000000001001'::uuid) ORDER BY \"workspace_members\".\"created_at\" DESC LIMIT 1"
      },
      {
        "sql": "SELECT \"workspace_members\".\"role\" FROM \"workspace_members\" WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = 'b942920ca7624f1889e60ed6f8702c7f'::uuid AND \"workspace_members\".\"workspace_id\" = '11111111222233334444000000001001'::uuid) ORDER BY \"workspace_members\".\"created_at\" DESC LIMIT 1"
      }
    ],
    "workspace_role_by_slug": [
      {
        "sql": "SELECT \"workspace_members\".\"role\" FROM \"workspace_members\" INNER JOIN \"workspaces\" ON (\"workspace_members\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = '615761be2c6c4a75abc9bed145879e9f'::uuid AND \"workspaces\".\"slug\" = 'fx2-workspace') ORDER BY \"workspace_members\".\"created_at\" DESC LIMIT 1"
      },
      {
        "sql": "SELECT \"workspace_members\".\"role\" FROM \"workspace_members\" INNER JOIN \"workspaces\" ON (\"workspace_members\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = '9db9237f42274603a0c70cef71824dad'::uuid AND \"workspaces\".\"slug\" = 'fx2-workspace') ORDER BY \"workspace_members\".\"created_at\" DESC LIMIT 1"
      },
      {
        "sql": "SELECT \"workspace_members\".\"role\" FROM \"workspace_members\" INNER JOIN \"workspaces\" ON (\"workspace_members\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = 'c24bd2ec9378435d847682dcd940b609'::uuid AND \"workspaces\".\"slug\" = 'fx2-workspace') ORDER BY \"workspace_members\".\"created_at\" DESC LIMIT 1"
      },
      {
        "sql": "SELECT \"workspace_members\".\"role\" FROM \"workspace_members\" INNER JOIN \"workspaces\" ON (\"workspace_members\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = '7233fbaabf55448ea615d28408d24335'::uuid AND \"workspaces\".\"slug\" = 'fx2-workspace') ORDER BY \"workspace_members\".\"created_at\" DESC LIMIT 1"
      },
      {
        "sql": "SELECT \"workspace_members\".\"role\" FROM \"workspace_members\" INNER JOIN \"workspaces\" ON (\"workspace_members\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = 'b942920ca7624f1889e60ed6f8702c7f'::uuid AND \"workspaces\".\"slug\" = 'fx2-workspace') ORDER BY \"workspace_members\".\"created_at\" DESC LIMIT 1"
      }
    ],
    "is_workspace_admin": [
      {
        "sql": "SELECT \"workspace_members\".\"role\" FROM \"workspace_members\" WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = '615761be2c6c4a75abc9bed145879e9f'::uuid AND \"workspace_members\".\"workspace_id\" = '11111111222233334444000000001001'::uuid) ORDER BY \"workspace_members\".\"created_at\" DESC LIMIT 1"
      },
      {
        "sql": "SELECT \"workspace_members\".\"role\" FROM \"workspace_members\" WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = '9db9237f42274603a0c70cef71824dad'::uuid AND \"workspace_members\".\"workspace_id\" = '11111111222233334444000000001001'::uuid) ORDER BY \"workspace_members\".\"created_at\" DESC LIMIT 1"
      },
      {
        "sql": "SELECT \"workspace_members\".\"role\" FROM \"workspace_members\" WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = 'c24bd2ec9378435d847682dcd940b609'::uuid AND \"workspace_members\".\"workspace_id\" = '11111111222233334444000000001001'::uuid) ORDER BY \"workspace_members\".\"created_at\" DESC LIMIT 1"
      },
      {
        "sql": "SELECT \"workspace_members\".\"role\" FROM \"workspace_members\" WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = '7233fbaabf55448ea615d28408d24335'::uuid AND \"workspace_members\".\"workspace_id\" = '11111111222233334444000000001001'::uuid) ORDER BY \"workspace_members\".\"created_at\" DESC LIMIT 1"
      },
      {
        "sql": "SELECT \"workspace_members\".\"role\" FROM \"workspace_members\" WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = 'b942920ca7624f1889e60ed6f8702c7f'::uuid AND \"workspace_members\".\"workspace_id\" = '11111111222233334444000000001001'::uuid) ORDER BY \"workspace_members\".\"created_at\" DESC LIMIT 1"
      }
    ],
    "is_at_least_member": [
      {
        "sql": "SELECT \"workspace_members\".\"role\" FROM \"workspace_members\" WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = '615761be2c6c4a75abc9bed145879e9f'::uuid AND \"workspace_members\".\"workspace_id\" = '11111111222233334444000000001001'::uuid) ORDER BY \"workspace_members\".\"created_at\" DESC LIMIT 1"
      },
      {
        "sql": "SELECT \"workspace_members\".\"role\" FROM \"workspace_members\" WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = '9db9237f42274603a0c70cef71824dad'::uuid AND \"workspace_members\".\"workspace_id\" = '11111111222233334444000000001001'::uuid) ORDER BY \"workspace_members\".\"created_at\" DESC LIMIT 1"
      },
      {
        "sql": "SELECT \"workspace_members\".\"role\" FROM \"workspace_members\" WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = 'c24bd2ec9378435d847682dcd940b609'::uuid AND \"workspace_members\".\"workspace_id\" = '11111111222233334444000000001001'::uuid) ORDER BY \"workspace_members\".\"created_at\" DESC LIMIT 1"
      },
      {
        "sql": "SELECT \"workspace_members\".\"role\" FROM \"workspace_members\" WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = '7233fbaabf55448ea615d28408d24335'::uuid AND \"workspace_members\".\"workspace_id\" = '11111111222233334444000000001001'::uuid) ORDER BY \"workspace_members\".\"created_at\" DESC LIMIT 1"
      },
      {
        "sql": "SELECT \"workspace_members\".\"role\" FROM \"workspace_members\" WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = 'b942920ca7624f1889e60ed6f8702c7f'::uuid AND \"workspace_members\".\"workspace_id\" = '11111111222233334444000000001001'::uuid) ORDER BY \"workspace_members\".\"created_at\" DESC LIMIT 1"
      }
    ]
  },
  "check_project_role_sql": {
    "member15 allowed=[15]": [
      {
        "sql": "SELECT 1 AS \"a\" FROM \"project_members\" INNER JOIN \"workspaces\" ON (\"project_members\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"project_members\".\"deleted_at\" IS NULL AND \"project_members\".\"is_active\" AND \"project_members\".\"member_id\" = '9db9237f42274603a0c70cef71824dad'::uuid AND \"project_members\".\"project_id\" = '11111111222233334444000000002001'::uuid AND \"project_members\".\"role\" IN (15) AND \"workspaces\".\"slug\" = 'fx2-workspace') LIMIT 1"
      }
    ],
    "member15 allowed=[20]": [
      {
        "sql": "SELECT 1 AS \"a\" FROM \"project_members\" INNER JOIN \"workspaces\" ON (\"project_members\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"project_members\".\"deleted_at\" IS NULL AND \"project_members\".\"is_active\" AND \"project_members\".\"member_id\" = '9db9237f42274603a0c70cef71824dad'::uuid AND \"project_members\".\"project_id\" = '11111111222233334444000000002001'::uuid AND \"project_members\".\"role\" IN (20) AND \"workspaces\".\"slug\" = 'fx2-workspace') LIMIT 1"
      },
      {
        "sql": "SELECT 1 AS \"a\" FROM \"project_members\" INNER JOIN \"workspaces\" ON (\"project_members\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"project_members\".\"deleted_at\" IS NULL AND \"project_members\".\"is_active\" AND \"project_members\".\"member_id\" = '9db9237f42274603a0c70cef71824dad'::uuid AND \"project_members\".\"project_id\" = '11111111222233334444000000002001'::uuid AND \"workspaces\".\"slug\" = 'fx2-workspace') LIMIT 1"
      },
      {
        "sql": "SELECT 1 AS \"a\" FROM \"workspace_members\" INNER JOIN \"workspaces\" ON (\"workspace_members\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = '9db9237f42274603a0c70cef71824dad'::uuid AND \"workspace_members\".\"role\" = 20 AND \"workspaces\".\"slug\" = 'fx2-workspace') LIMIT 1"
      }
    ],
    "member15 allowed=[20] no-bypass": [
      {
        "sql": "SELECT 1 AS \"a\" FROM \"project_members\" INNER JOIN \"workspaces\" ON (\"project_members\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"project_members\".\"deleted_at\" IS NULL AND \"project_members\".\"is_active\" AND \"project_members\".\"member_id\" = '9db9237f42274603a0c70cef71824dad'::uuid AND \"project_members\".\"project_id\" = '11111111222233334444000000002001'::uuid AND \"project_members\".\"role\" IN (20) AND \"workspaces\".\"slug\" = 'fx2-workspace') LIMIT 1"
      }
    ],
    "admin5 allowed=[15] bypass": [
      {
        "sql": "SELECT 1 AS \"a\" FROM \"project_members\" INNER JOIN \"workspaces\" ON (\"project_members\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"project_members\".\"deleted_at\" IS NULL AND \"project_members\".\"is_active\" AND \"project_members\".\"member_id\" = '615761be2c6c4a75abc9bed145879e9f'::uuid AND \"project_members\".\"project_id\" = '11111111222233334444000000002001'::uuid AND \"project_members\".\"role\" IN (15) AND \"workspaces\".\"slug\" = 'fx2-workspace') LIMIT 1"
      },
      {
        "sql": "SELECT 1 AS \"a\" FROM \"project_members\" INNER JOIN \"workspaces\" ON (\"project_members\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"project_members\".\"deleted_at\" IS NULL AND \"project_members\".\"is_active\" AND \"project_members\".\"member_id\" = '615761be2c6c4a75abc9bed145879e9f'::uuid AND \"project_members\".\"project_id\" = '11111111222233334444000000002001'::uuid AND \"workspaces\".\"slug\" = 'fx2-workspace') LIMIT 1"
      },
      {
        "sql": "SELECT 1 AS \"a\" FROM \"workspace_members\" INNER JOIN \"workspaces\" ON (\"workspace_members\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = '615761be2c6c4a75abc9bed145879e9f'::uuid AND \"workspace_members\".\"role\" = 20 AND \"workspaces\".\"slug\" = 'fx2-workspace') LIMIT 1"
      }
    ],
    "admin5 allowed=[15] no-bypass": [
      {
        "sql": "SELECT 1 AS \"a\" FROM \"project_members\" INNER JOIN \"workspaces\" ON (\"project_members\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"project_members\".\"deleted_at\" IS NULL AND \"project_members\".\"is_active\" AND \"project_members\".\"member_id\" = '615761be2c6c4a75abc9bed145879e9f'::uuid AND \"project_members\".\"project_id\" = '11111111222233334444000000002001'::uuid AND \"project_members\".\"role\" IN (15) AND \"workspaces\".\"slug\" = 'fx2-workspace') LIMIT 1"
      }
    ],
    "guest5 allowed=[15,20] bypass": [
      {
        "sql": "SELECT 1 AS \"a\" FROM \"project_members\" INNER JOIN \"workspaces\" ON (\"project_members\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"project_members\".\"deleted_at\" IS NULL AND \"project_members\".\"is_active\" AND \"project_members\".\"member_id\" = 'c24bd2ec9378435d847682dcd940b609'::uuid AND \"project_members\".\"project_id\" = '11111111222233334444000000002001'::uuid AND \"project_members\".\"role\" IN (15, 20) AND \"workspaces\".\"slug\" = 'fx2-workspace') LIMIT 1"
      },
      {
        "sql": "SELECT 1 AS \"a\" FROM \"project_members\" INNER JOIN \"workspaces\" ON (\"project_members\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"project_members\".\"deleted_at\" IS NULL AND \"project_members\".\"is_active\" AND \"project_members\".\"member_id\" = 'c24bd2ec9378435d847682dcd940b609'::uuid AND \"project_members\".\"project_id\" = '11111111222233334444000000002001'::uuid AND \"workspaces\".\"slug\" = 'fx2-workspace') LIMIT 1"
      },
      {
        "sql": "SELECT 1 AS \"a\" FROM \"workspace_members\" INNER JOIN \"workspaces\" ON (\"workspace_members\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"workspace_members\".\"deleted_at\" IS NULL AND \"workspace_members\".\"is_active\" AND \"workspace_members\".\"member_id\" = 'c24bd2ec9378435d847682dcd940b609'::uuid AND \"workspace_members\".\"role\" = 20 AND \"workspaces\".\"slug\" = 'fx2-workspace') LIMIT 1"
      }
    ],
    "outsider allowed=[5,15,20]": [
      {
        "sql": "SELECT 1 AS \"a\" FROM \"project_members\" INNER JOIN \"workspaces\" ON (\"project_members\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"project_members\".\"deleted_at\" IS NULL AND \"project_members\".\"is_active\" AND \"project_members\".\"member_id\" = 'b942920ca7624f1889e60ed6f8702c7f'::uuid AND \"project_members\".\"project_id\" = '11111111222233334444000000002001'::uuid AND \"project_members\".\"role\" IN (5, 15, 20) AND \"workspaces\".\"slug\" = 'fx2-workspace') LIMIT 1"
      },
      {
        "sql": "SELECT 1 AS \"a\" FROM \"project_members\" INNER JOIN \"workspaces\" ON (\"project_members\".\"workspace_id\" = \"workspaces\".\"id\") WHERE (\"project_members\".\"deleted_at\" IS NULL AND \"project_members\".\"is_active\" AND \"project_members\".\"member_id\" = 'b942920ca7624f1889e60ed6f8702c7f'::uuid AND \"project_members\".\"project_id\" = '11111111222233334444000000002001'::uuid AND \"workspaces\".\"slug\" = 'fx2-workspace') LIMIT 1"
      }
    ],
    "anonymous allowed=[15]": [],
    "none allowed=[15]": []
  }
}
