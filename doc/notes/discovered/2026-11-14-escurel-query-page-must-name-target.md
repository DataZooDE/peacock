# escurel refuses a query page whose SQL never names `{{target}}`

**Symptom.** A compiled dashboard heading (`SELECT 'Northwind revenue'::LABEL;`, no FROM) became a
query page that escurel rejected at `query_instance`:
``[[query::nw-dashboard-q1]] `sql` never names `{{target}}` — a statement that does not use the
target it was authorised against cannot run``.

**Fix.** `peacock author compile` rewrites a constant statement (no `FROM` outside literals) to
read one row of the target: `<statement> FROM {{target}} LIMIT 1`. A statement that reads some
*other* table is a compile error — a query page may only read its authorised managed view.

**Recognise it.** Any escurel query page must reference `{{target}}`; author constant panels
(headings, notes) knowing they still cost one bounded read of the view.
