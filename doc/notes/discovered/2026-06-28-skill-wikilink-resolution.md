# Resolving a report skill page by `[[skill::<id>]]`

Early on, only the bare `[[<id>]]` form resolved a skill *page* (a skill page's
`skill` column is its own id, so `[[skill::<id>]]` matched nothing). peacock
used the bare form as a workaround.

**RESOLVED (escurel #212, commit a6f96e1):** escurel now treats `skill::` as a
reserved namespace meaning "the skill definition page", so `[[skill::<id>]]`
resolves it. peacock's `resolve_report` uses the typed form.
