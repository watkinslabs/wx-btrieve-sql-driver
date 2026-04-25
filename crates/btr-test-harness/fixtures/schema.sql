USE master;
GO
IF DB_ID('WXBTRV_TEST') IS NOT NULL
BEGIN
    ALTER DATABASE WXBTRV_TEST SET SINGLE_USER WITH ROLLBACK IMMEDIATE;
    DROP DATABASE WXBTRV_TEST;
END
IF DB_ID('WXBTRV_TEST') IS NULL
    CREATE DATABASE WXBTRV_TEST;
GO
USE WXBTRV_TEST;
GO

CREATE TABLE TEST_CUST (
    MDS_RECNUM  INT IDENTITY(1,1) PRIMARY KEY,
    CUST_ID     CHAR(8)       NOT NULL,
    CUST_NAME   CHAR(30)      NOT NULL,
    CITY        CHAR(20)      NOT NULL DEFAULT '',
    STATE       CHAR(2)       NOT NULL DEFAULT '',
    BALANCE     BIGINT        NOT NULL DEFAULT 0,
    ACTIVE      TINYINT       NOT NULL DEFAULT 0,
    CREATED     DATE          NULL
);
GO
CREATE UNIQUE INDEX IX_CUST_ID ON TEST_CUST(CUST_ID);
GO
CREATE INDEX IX_CUST_NAME ON TEST_CUST(CUST_NAME);
GO

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
GO

-- ── TEST_MULTI: compound key + descending single-segment ────────────────────
CREATE TABLE TEST_MULTI (
    REGION      CHAR(4)  NOT NULL,
    DEPT        CHAR(4)  NOT NULL,
    SUB_CODE    CHAR(6)  NOT NULL,
    VALUE       INT      NOT NULL DEFAULT 0,
    MDS_RECNUM  INT IDENTITY(1,1) PRIMARY KEY
);
GO
CREATE UNIQUE INDEX IX_MULTI_KEY ON TEST_MULTI(REGION, DEPT, SUB_CODE);
GO
CREATE INDEX IX_MULTI_VALUE_DESC ON TEST_MULTI(VALUE DESC);
GO
-- 12 rows: 3 regions * 2 depts * 2 sub_codes; insertion order != sorted order
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
GO

-- ── TEST_AUTOINC: AUTO_ID is the Btrieve AUTOINCREMENT + SQL identity ──────
CREATE TABLE TEST_AUTOINC (
    AUTO_ID  INT IDENTITY(1,1) PRIMARY KEY,
    NAME     CHAR(20) NOT NULL DEFAULT ''
);
GO
INSERT INTO TEST_AUTOINC (NAME) VALUES
('ALPHA               '),
('BRAVO               '),
('CHARLIE             '),
('DELTA               '),
('ECHO                ');
GO

-- ── TEST_TYPES: one of each Btrieve native type ────────────────────────────
CREATE TABLE TEST_TYPES (
    MDS_RECNUM INT IDENTITY(1,1) PRIMARY KEY,
    STR_FIX   CHAR(10) NOT NULL DEFAULT '',
    STR_Z     CHAR(16) NOT NULL DEFAULT '',
    INT_VAL   INT      NOT NULL DEFAULT 0,
    DEC_VAL   BIGINT   NOT NULL DEFAULT 0,
    LOG_VAL   TINYINT  NOT NULL DEFAULT 0,
    DATE_VAL  DATE     NULL
);
GO
CREATE UNIQUE INDEX IX_TYPES_STR_FIX ON TEST_TYPES(STR_FIX);
GO
CREATE INDEX IX_TYPES_INT_VAL ON TEST_TYPES(INT_VAL);
GO
INSERT INTO TEST_TYPES (STR_FIX, STR_Z, INT_VAL, DEC_VAL, LOG_VAL, DATE_VAL) VALUES
('AAAA      ', 'first',           100,   123456,  1, '2024-01-15'),
('BBBB      ', 'second value',    200,   999999,  0, '2024-02-20'),
('CCCC      ', 'third',          -250,        0,  1, NULL),
('DDDD      ', 'fourth row',        0,   555555,  1, '2024-04-05'),
('EEEE      ', 'final',          1000, 12345678,  0, '2024-05-12'),
('FFFF      ', 'sixth!',           50,    42424,  1, '2024-06-18');
GO

-- ── TEST_DESC: descending single-segment key ───────────────────────────────
CREATE TABLE TEST_DESC (
    RANK_VAL    INT      NOT NULL,
    LABEL       CHAR(8)  NOT NULL DEFAULT '',
    MDS_RECNUM  INT IDENTITY(1,1) PRIMARY KEY
);
GO
CREATE INDEX IX_DESC_RANK ON TEST_DESC(RANK_VAL DESC);
GO
INSERT INTO TEST_DESC (RANK_VAL, LABEL) VALUES
(50, 'fifty   '),
(20, 'twenty  '),
(80, 'eighty  '),
(10, 'ten     '),
(60, 'sixty   '),
(30, 'thirty  '),
(70, 'seventy '),
(40, 'forty   ');
GO
