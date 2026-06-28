# escurel serializes a DuckDB DATE column as `Date32(<days>)`

**Symptom.** A `query_instance` whose projection includes a bare `DATE`
column (e.g. `date_trunc('month', order_date)::DATE AS month`) returns that
column in `rows` as the string `"Date32(9862)"` — the Rust `Debug` form of
DuckDB's `Value::Date32(days_since_epoch)` — not an ISO date. `9862` is days
after 1970-01-01, i.e. 1997-01-01.

**Fix.** Cast date/time columns to text **inside the query SQL** so the row
JSON carries a clean string:

```sql
strftime(date_trunc('month', order_date), '%Y-%m-%d') AS month
```

This also feeds Vega-Lite's `temporal` x-axis directly (ISO strings parse
natively), so it is the right shape for the report anyway.

**Recognise it next time.** Any `rows[*][col]` that comes back as
`"Date32(...)"`, `"Time64(...)"`, `"Timestamp(...)"`, etc. means the query
projected a raw temporal type — wrap it in `strftime`/`::VARCHAR` in the
query page's `sql`. peacock treats escurel rows as already-shaped for
rendering (FR-D-4); shaping temporal columns is the query author's job.

---
**RESOLVED (escurel #211, commit 5e63007):** escurel now serializes
DATE/TIME/TIMESTAMP/INTERVAL as ISO-8601 strings (a DATE → `"1997-01-01"`).
peacock dropped the `strftime` workaround — the Northwind query projects
`date_trunc('month', order_date)::DATE AS month` directly.
