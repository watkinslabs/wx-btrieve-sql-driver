-- SQLite fixture for the btr-test-harness.
--
-- Same shape as schema.sql (the MSSQL fixture) but with SQLite-portable
-- DDL: TEXT for fixed-width chars, INTEGER PRIMARY KEY AUTOINCREMENT for
-- the recnum/identity column. The harness's reset_sqlite_fixture()
-- DROP TABLE IF EXISTS each fixture table before re-running this.

DROP TABLE IF EXISTS TEST_CUST;
DROP TABLE IF EXISTS TEST_MULTI;
DROP TABLE IF EXISTS TEST_AUTOINC;
DROP TABLE IF EXISTS TEST_TYPES;
DROP TABLE IF EXISTS TEST_DESC;

CREATE TABLE TEST_CUST (
    MDS_RECNUM  INTEGER PRIMARY KEY AUTOINCREMENT,
    CUST_ID     TEXT    NOT NULL,
    CUST_NAME   TEXT    NOT NULL,
    CITY        TEXT    NOT NULL DEFAULT '',
    STATE       TEXT    NOT NULL DEFAULT '',
    BALANCE     INTEGER NOT NULL DEFAULT 0,
    ACTIVE      INTEGER NOT NULL DEFAULT 0,
    CREATED     TEXT
);
CREATE UNIQUE INDEX IX_CUST_ID ON TEST_CUST(CUST_ID);
CREATE INDEX IX_CUST_NAME ON TEST_CUST(CUST_NAME);

INSERT INTO TEST_CUST (CUST_ID, CUST_NAME, CITY, STATE, BALANCE, ACTIVE, CREATED) VALUES
('A0001   ', 'ACME INDUSTRIES               ', 'SPRINGFIELD         ', 'IL',  100000, 1, '2024-01-15'),
('A0002   ', 'BETA CORP                     ', 'PORTLAND            ', 'OR',  250050, 1, '2024-02-20'),
('A0003   ', 'CRANE WORKS                   ', 'AUSTIN              ', 'TX',   75000, 1, '2024-03-10'),
('A0004   ', 'DELTA SHIPPING                ', 'MIAMI               ', 'FL',  500000, 0, '2024-04-05'),
('A0005   ', 'ECHO SOFTWARE                 ', 'SEATTLE             ', 'WA', 1250075, 1, '2024-05-12'),
('A0006   ', 'FOXTROT LLC                   ', 'DENVER              ', 'CO',   25025, 1, '2024-06-18'),
('A0007   ', 'GOLF PARTNERS                 ', 'PHOENIX             ', 'AZ',       0, 0, '2024-07-22'),
('A0008   ', 'HOTEL MANAGEMENT              ', 'LAS VEGAS           ', 'NV',  875040, 1, '2024-08-30'),
('A0009   ', 'INDIA BATCH                   ', 'BOSTON              ', 'MA',  320000, 1, '2024-09-14'),
('A0010   ', 'JULIET FURNITURE              ', 'ATLANTA             ', 'GA',   99999, 1, '2024-10-25');

CREATE TABLE TEST_MULTI (
    REGION      TEXT    NOT NULL,
    DEPT        TEXT    NOT NULL,
    SUB_CODE    TEXT    NOT NULL,
    VALUE       INTEGER NOT NULL DEFAULT 0,
    MDS_RECNUM  INTEGER PRIMARY KEY AUTOINCREMENT
);
CREATE UNIQUE INDEX IX_MULTI_KEY ON TEST_MULTI(REGION, DEPT, SUB_CODE);
CREATE INDEX IX_MULTI_VALUE_DESC ON TEST_MULTI(VALUE DESC);
INSERT INTO TEST_MULTI (REGION, DEPT, SUB_CODE, VALUE) VALUES
('WEST', 'ENG ', 'BETA  ',  500),
('EAST', 'OPS ', 'ALPHA ',  900),
('SOUT', 'ENG ', 'ALPHA ',  100),
('WEST', 'OPS ', 'ALPHA ',  750),
('EAST', 'ENG ', 'BETA  ',  300),
('SOUT', 'OPS ', 'BETA  ',  450),
('WEST', 'ENG ', 'ALPHA ',  200),
('EAST', 'ENG ', 'ALPHA ',  600),
('SOUT', 'OPS ', 'ALPHA ',  850),
('WEST', 'OPS ', 'BETA  ',  150),
('EAST', 'OPS ', 'BETA  ',  400),
('SOUT', 'ENG ', 'BETA  ',  700);

CREATE TABLE TEST_AUTOINC (
    AUTO_ID  INTEGER PRIMARY KEY AUTOINCREMENT,
    NAME     TEXT NOT NULL DEFAULT ''
);
INSERT INTO TEST_AUTOINC (NAME) VALUES
('ALPHA               '),
('BRAVO               '),
('CHARLIE             '),
('DELTA               '),
('ECHO                ');

CREATE TABLE TEST_TYPES (
    MDS_RECNUM INTEGER PRIMARY KEY AUTOINCREMENT,
    STR_FIX   TEXT    NOT NULL DEFAULT '',
    STR_Z     TEXT    NOT NULL DEFAULT '',
    INT_VAL   INTEGER NOT NULL DEFAULT 0,
    DEC_VAL   INTEGER NOT NULL DEFAULT 0,
    LOG_VAL   INTEGER NOT NULL DEFAULT 0,
    DATE_VAL  TEXT
);
CREATE UNIQUE INDEX IX_TYPES_STR_FIX ON TEST_TYPES(STR_FIX);
CREATE INDEX IX_TYPES_INT_VAL ON TEST_TYPES(INT_VAL);
INSERT INTO TEST_TYPES (STR_FIX, STR_Z, INT_VAL, DEC_VAL, LOG_VAL, DATE_VAL) VALUES
('AAAA      ', 'first',           100,   123456,  1, '2024-01-15'),
('BBBB      ', 'second value',    200,   999999,  0, '2024-02-20'),
('CCCC      ', 'third',          -250,        0,  1, NULL),
('DDDD      ', 'fourth row',        0,   555555,  1, '2024-04-05'),
('EEEE      ', 'final',          1000, 12345678,  0, '2024-05-12'),
('FFFF      ', 'sixth!',           50,    42424,  1, '2024-06-18');

CREATE TABLE TEST_DESC (
    RANK_VAL    INTEGER NOT NULL,
    LABEL       TEXT    NOT NULL DEFAULT '',
    MDS_RECNUM  INTEGER PRIMARY KEY AUTOINCREMENT
);
CREATE INDEX IX_DESC_RANK ON TEST_DESC(RANK_VAL DESC);
INSERT INTO TEST_DESC (RANK_VAL, LABEL) VALUES
(50, 'fifty   '),
(20, 'twenty  '),
(80, 'eighty  '),
(10, 'ten     '),
(60, 'sixty   '),
(30, 'thirty  '),
(70, 'seventy '),
(40, 'forty   ');
