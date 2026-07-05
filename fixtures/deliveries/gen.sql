-- Supplier-deliveries fixture generator (deterministic, offline).
--
-- One row per completed inbound delivery: the supplier, the day it arrived,
-- and the realised lead time in days. This is the shape escurel's
-- `parquet_dir` connector exposes read-only as the `supplier_deliveries`
-- structured data view; the `sd_lead_times` query page reads it with
-- `:from`/`:to` bound as prepared-statement parameters, and the
-- `supplier-lead-times` report skill draws the lead-time DENSITY with the
-- contracted 14-day SLA as a `vline` and the computed `p90` marker — the
-- ggplot statistical backend's acceptance chart (peacock #8).
--
-- Three suppliers, hand-authored plausible lead times (1997, one year):
--   alpine    — fast and tight        (7–13 days, well inside contract)
--   borealis  — mid, occasional slip  (9–17 days)
--   cormorant — slow with a long tail (11–26 days, drags the p90 past 14)
--
-- Regenerate with:  duckdb < fixtures/deliveries/gen.sql   (run from repo root)
-- The resulting parquet is committed so tests need no generation step.

COPY (
  SELECT
    supplier                AS supplier,
    delivered_on::DATE      AS delivered_on,
    actual_days::DOUBLE     AS actual_days
  FROM (VALUES
    -- alpine ------------------------------------------------------------
    ('alpine',    DATE '1997-01-08',  8.0),
    ('alpine',    DATE '1997-02-03',  7.0),
    ('alpine',    DATE '1997-03-12', 10.0),
    ('alpine',    DATE '1997-04-02',  9.0),
    ('alpine',    DATE '1997-05-06', 11.0),
    ('alpine',    DATE '1997-06-11',  8.0),
    ('alpine',    DATE '1997-07-09', 12.0),
    ('alpine',    DATE '1997-08-04',  9.0),
    ('alpine',    DATE '1997-09-15', 13.0),
    ('alpine',    DATE '1997-10-01',  8.0),
    ('alpine',    DATE '1997-11-05', 10.0),
    ('alpine',    DATE '1997-12-10',  9.0),

    -- borealis ----------------------------------------------------------
    ('borealis',  DATE '1997-01-20', 11.0),
    ('borealis',  DATE '1997-02-14',  9.0),
    ('borealis',  DATE '1997-03-25', 13.0),
    ('borealis',  DATE '1997-04-16', 12.0),
    ('borealis',  DATE '1997-05-21', 15.0),
    ('borealis',  DATE '1997-06-18', 10.0),
    ('borealis',  DATE '1997-07-23', 14.0),
    ('borealis',  DATE '1997-08-13', 12.0),
    ('borealis',  DATE '1997-09-24', 17.0),
    ('borealis',  DATE '1997-10-15', 11.0),
    ('borealis',  DATE '1997-11-19', 13.0),
    ('borealis',  DATE '1997-12-17', 16.0),

    -- cormorant ---------------------------------------------------------
    ('cormorant', DATE '1997-01-29', 14.0),
    ('cormorant', DATE '1997-02-26', 12.0),
    ('cormorant', DATE '1997-03-31', 18.0),
    ('cormorant', DATE '1997-04-28', 15.0),
    ('cormorant', DATE '1997-05-30', 22.0),
    ('cormorant', DATE '1997-06-27', 13.0),
    ('cormorant', DATE '1997-07-30', 19.0),
    ('cormorant', DATE '1997-08-27', 16.0),
    ('cormorant', DATE '1997-09-29', 26.0),
    ('cormorant', DATE '1997-10-29', 14.0),
    ('cormorant', DATE '1997-11-26', 17.0),
    ('cormorant', DATE '1997-12-30', 11.0)
  ) AS t(supplier, delivered_on, actual_days)
) TO 'fixtures/deliveries/supplier_deliveries/data.parquet' (FORMAT parquet);
