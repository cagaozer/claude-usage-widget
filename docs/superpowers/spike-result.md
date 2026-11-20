# Spike result — 2026-11-20

`cargo run --example spike` against a real Claude Pro session (one org):

| Request | Status | Content-Type | Cloudflare challenge |
|---|---|---|---|
| `GET /api/organizations` | 200 | application/json | no |
| `GET /api/organizations/{uuid}/usage` | 200 | application/json | no |

**BACKEND=http** — plain HTTPS with honest User-Agent and `Cookie: sessionKey=…` works.
Task 4b (hidden WebView2 backend) is not needed.

## Observed usage shape (fixture: `tests/fixtures/usage_pro.json`)

- `five_hour` / `seven_day`: objects `{ "utilization": 39.0, "resets_at": "2026-11-21T03:50:00.185445+00:00", ... }`
  - utilization is **percent on a 0–100 scale** (f64).
  - resets_at is RFC 3339 with microseconds and `+00:00` offset.
- Per-model weekly windows: `seven_day_opus`, `seven_day_sonnet` (null for this Pro account),
  also `seven_day_cowork`, `seven_day_oauth_apps`, `seven_day_omelette` (null).
- Also present but unused: `limits[]` (`kind`: session / weekly_all, integer `percent`),
  `seven_day_breakdown`, `extra_usage`, `spend`, many null codenamed fields.
- Field names match the spec's assumptions; Task 2 uses `five_hour`, `seven_day`, and
  `seven_day_opus` / `seven_day_sonnet` as the optional extra rows.

## Orgs

`/api/organizations` returns an array; each org has `uuid`, `name`, `capabilities`
(e.g. `["claude_pro","chat"]`). The committed `orgs.json` fixture is hand-minimized to
those fields (the real response contains account ids and settings).
