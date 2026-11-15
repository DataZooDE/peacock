# anofox drops a whole panel, silently, when a layout role sits in a data statement

**Symptom.** In an anofox `dashboard.sql`, `SELECT category::CATEGORY, sum(x)::DONUT, 8::COL FROM t …`
renders nothing for that statement — no panel, no error. anofox treats any statement carrying a
layout role (`::COL`, `::COLUMNS`, `::GROUP`, …) as a layout directive, so the chart is never drawn.

**Fix.** `peacock author compile` runs anofox's own linter as a hard gate, *without a database*:
`lint::check_opts(script, |_| Ok(vec![]), LintOptions { design: false, .. })`. The static checks
still fire — this one is `layout/inline-layout-role` with the statement number — while the
data-dependent codes (`empty-panel`, `sql-error`, `render-error`, `render-warning`,
`silent-setup`) are ignored because every read returns no rows. At render time,
`peacock_core::dashboard` additionally fails if a compiled `draws: true` statement was never read
(the same drop caused by a script that drifted from its compiled pages). For single `kind: anofox`
specs, `chart::check_spec_shape` rejects layout/input/chrome roles outright.

**Recognise it.** A dashboard renders with fewer panels than statements and nothing in the logs.
Put `SELECT n::COL;` on its own line *before* the panel instead.
