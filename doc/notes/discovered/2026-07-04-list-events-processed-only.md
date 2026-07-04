# escurel `list_events` returns only PROCESSED events (timeline views)

**Symptom.** A `timeline` view renders empty although events were visibly
captured against the instance (`capture_event` succeeded, the webhook fired,
the event shows in `list_inbox`).

**Cause.** escurel separates the **inbox** (pending work items,
`status='inbox'`) from an instance's **history** (`list_events`, which
returns only `status='processed'` events, oldest first). A captured event
becomes history only when a consumer `assign_event`s it to the instance.
That is by design: the inbox is a work queue, the history is the folded
outcome.

**Fix / rule.** Every producer that wants its event to show on a peacock
timeline must `capture_event` **and** `assign_event` (immediately, when
there is no worker in the loop). The timeline tests pin this:
`instance_timeline.rs::timeline_limit_caps_and_unassigned_events_are_invisible`.

**How to recognise it.** Timeline empty + the event still listed by
`list_inbox` → nobody assigned it.
