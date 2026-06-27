-- Northwind order-lines fixture generator (deterministic, offline).
--
-- Produces a denormalized "order lines" relation — one row per order detail,
-- carrying the order date, the product's category name, and the line's
-- price/quantity/discount. This is the shape escurel's `parquet_dir`
-- connector exposes read-only as the `nw_order_lines` structured data view;
-- the `nw_revenue_by_category` query page then does the real GROUP BY
-- aggregation (revenue = unit_price * quantity * (1 - discount)) with the
-- report's `:from` / `:to` / `:category` bound as prepared-statement params.
--
-- Regenerate with:  duckdb < fixtures/northwind/gen.sql
-- The resulting parquet is committed so tests need no generation step.

COPY (
  SELECT
    order_date::DATE        AS order_date,
    category                AS category,
    unit_price::DOUBLE      AS unit_price,
    quantity::INTEGER       AS quantity,
    discount::DOUBLE        AS discount
  FROM (VALUES
    -- 1996 lines (excluded by a from=1997-01-01 filter) ------------------
    (DATE '1996-12-15', 'Beverages',      10.0, 10, 0.0),
    (DATE '1996-11-20', 'Seafood',        20.0,  5, 0.0),

    -- 1997 Q1 -----------------------------------------------------------
    (DATE '1997-01-10', 'Beverages',      18.0, 10, 0.0),   -- 180
    (DATE '1997-01-15', 'Condiments',     22.0,  5, 0.0),   -- 110
    (DATE '1997-02-05', 'Beverages',      18.0,  5, 0.1),   -- 81
    (DATE '1997-02-18', 'Dairy Products', 34.0, 10, 0.0),   -- 340
    (DATE '1997-03-03', 'Produce',        15.0, 20, 0.25),  -- 225
    (DATE '1997-03-22', 'Seafood',        25.0,  8, 0.0),   -- 200

    -- 1997 Q2 -----------------------------------------------------------
    (DATE '1997-04-09', 'Beverages',      18.0, 12, 0.0),   -- 216
    (DATE '1997-05-14', 'Condiments',     22.0, 10, 0.1),   -- 198
    (DATE '1997-06-01', 'Dairy Products', 34.0,  5, 0.0),   -- 170
    (DATE '1997-06-25', 'Seafood',        25.0, 10, 0.2),   -- 200

    -- 1997 Q3 -----------------------------------------------------------
    (DATE '1997-07-07', 'Beverages',      18.0,  8, 0.0),   -- 144
    (DATE '1997-08-19', 'Produce',        15.0, 10, 0.0),   -- 150
    (DATE '1997-09-30', 'Condiments',     22.0,  4, 0.0),   -- 88

    -- 1997 Q4 -----------------------------------------------------------
    (DATE '1997-10-10', 'Beverages',      18.0, 10, 0.0),   -- 180
    (DATE '1997-11-11', 'Dairy Products', 34.0,  6, 0.0),   -- 204
    (DATE '1997-12-24', 'Seafood',        25.0, 12, 0.0)    -- 300
  ) AS t(order_date, category, unit_price, quantity, discount)
) TO 'fixtures/northwind/order_lines/data.parquet' (FORMAT parquet);
