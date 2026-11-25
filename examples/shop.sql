-- Source for examples/shop.db. Rebuild with:
--   sqlite3 examples/shop.db < examples/shop.sql
DROP TABLE IF EXISTS customers;
CREATE TABLE customers (
    name    TEXT NOT NULL,
    age     INTEGER NOT NULL,
    city    TEXT,
    balance REAL NOT NULL DEFAULT 0
);
INSERT INTO customers (name, age, city, balance) VALUES
    ('Ann',   34, 'Olympia', 120.50),
    ('Bo',    17, 'Tacoma',    0.00),
    ('Cleo',  52, 'Seattle',  89.99),
    ('Dev',   16, NULL,        5.25),
    ('Esme',  29, 'Olympia',  42.00);
