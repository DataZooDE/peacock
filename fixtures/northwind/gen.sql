-- Northwind order-lines fixture generator (deterministic, offline).
--
-- One row per order detail. Carries the order date, the product's category and
-- name, the destination country, the salesperson, and the line's
-- price/quantity/discount. This is the shape escurel's `parquet_dir` connector
-- exposes read-only as the `nw_order_lines` structured data view; the query
-- pages then do the real GROUP BY aggregations (revenue = unit_price * quantity
-- * (1 - discount)) with each report's params bound as prepared-statement
-- parameters.
--
-- The order_date / category / unit_price / quantity / discount columns (and
-- therefore the revenue-by-category aggregation) are unchanged from the
-- original fixture, so the committed revenue numbers stay exact. The added
-- product / country / salesperson columns power the ranking, geography, and
-- discount scenarios — the category query ignores them.
--
-- Regenerate with:  duckdb < fixtures/northwind/gen.sql   (run from repo root)
-- The resulting parquet is committed so tests need no generation step.

COPY (
  SELECT
    order_date::DATE        AS order_date,
    category                AS category,
    product                 AS product,
    country                 AS country,
    salesperson             AS salesperson,
    unit_price::DOUBLE      AS unit_price,
    quantity::INTEGER       AS quantity,
    discount::DOUBLE        AS discount
  FROM (VALUES
    -- 1996 lines (excluded by a from=1997-01-01 filter) ------------------
    (DATE '1996-12-15', 'Beverages', 'Chai',  'Germany', 'Anne Becker', 10.0, 10, 0.0),
    (DATE '1996-11-20', 'Seafood',   'Ikura', 'Italy',   'Carla Diaz',  20.0,  5, 0.0),

    -- 1997 Q1 -----------------------------------------------------------
    (DATE '1997-01-10', 'Beverages',      'Chai',          'Germany', 'Anne Becker', 18.0, 10, 0.0),   -- 180
    (DATE '1997-01-15', 'Condiments',     'Aniseed Syrup', 'UK',      'Bob Curtis',  22.0,  5, 0.0),   -- 110
    (DATE '1997-02-05', 'Beverages',      'Chang',         'France',  'Carla Diaz',  18.0,  5, 0.1),   -- 81
    (DATE '1997-02-18', 'Dairy Products', 'Gorgonzola',    'Germany', 'Anne Becker', 34.0, 10, 0.0),   -- 340
    (DATE '1997-03-03', 'Produce',        'Tofu',          'Spain',   'Bob Curtis',  15.0, 20, 0.25),  -- 225
    (DATE '1997-03-22', 'Seafood',        'Ikura',         'Italy',   'Carla Diaz',  25.0,  8, 0.0),   -- 200

    -- 1997 Q2 -----------------------------------------------------------
    (DATE '1997-04-09', 'Beverages',      'Chai',          'Germany', 'Anne Becker', 18.0, 12, 0.0),   -- 216
    (DATE '1997-05-14', 'Condiments',     'Chef Anton''s', 'UK',      'Dan Evans',   22.0, 10, 0.1),   -- 198
    (DATE '1997-06-01', 'Dairy Products', 'Mozzarella',    'France',  'Carla Diaz',  34.0,  5, 0.0),   -- 170
    (DATE '1997-06-25', 'Seafood',        'Konbu',         'Austria', 'Dan Evans',   25.0, 10, 0.2),   -- 200

    -- 1997 Q3 -----------------------------------------------------------
    (DATE '1997-07-07', 'Beverages',      'Chang',         'Germany', 'Anne Becker', 18.0,  8, 0.0),   -- 144
    (DATE '1997-08-19', 'Produce',        'Tofu',          'Spain',   'Bob Curtis',  15.0, 10, 0.0),   -- 150
    (DATE '1997-09-30', 'Condiments',     'Aniseed Syrup', 'Italy',   'Carla Diaz',  22.0,  4, 0.0),   -- 88

    -- 1997 Q4 -----------------------------------------------------------
    (DATE '1997-10-10', 'Beverages',      'Chai',          'Germany', 'Anne Becker', 18.0, 10, 0.0),   -- 180
    (DATE '1997-11-11', 'Dairy Products', 'Gorgonzola',    'UK',      'Dan Evans',   34.0,  6, 0.0),   -- 204
    (DATE '1997-12-24', 'Seafood',        'Ikura',         'France',  'Carla Diaz',  25.0, 12, 0.0)    -- 300
  ) AS t(order_date, category, product, country, salesperson, unit_price, quantity, discount)
) TO 'fixtures/northwind/order_lines/data.parquet' (FORMAT parquet);
