//! Per-backend SQL syntax. Centralizes every dialect difference so the op
//! layer can stay backend-agnostic.
//!
//! Today most ops still emit literal SQL Server syntax (`SELECT TOP n ...`,
//! `[bracket]` quoting, `IDENTITY(1,1)`, `SCOPE_IDENTITY()`). Step 2 of the
//! multi-backend rollout replaces those literals with `dialect()` calls.
//!
//! The active dialect is picked from `state().backend` at runtime via
//! [`active`].

use crate::state::{state, Backend};

/// Where in a `SELECT` to put the row limit.
#[derive(Copy, Clone, Debug)]
pub enum LimitKind {
    /// MSSQL: `SELECT TOP n ...` directly after `SELECT`.
    SelectTop,
    /// Postgres / SQLite: `... LIMIT n` at the end of the statement.
    LimitSuffix,
}

pub trait Dialect {
    /// Where the limit clause lives.
    fn limit_kind(&self) -> LimitKind;

    /// Quote an identifier (table or column name) for safe inclusion in SQL.
    /// MSSQL: `[name]`, Postgres / SQLite: `"name"`.
    fn quote_ident(&self, name: &str) -> String;

    /// Full column declaration for the recnum identity / autoincrement
    /// column in a CREATE TABLE — includes `PRIMARY KEY` when the dialect
    /// requires it inline (SQLite). Caller emits this verbatim after the
    /// column name.
    fn identity_column(&self) -> &'static str;

    /// `CREATE TABLE [IF NOT EXISTS] table_qualified (cols)` adapted to
    /// the dialect. MSSQL has no native IF NOT EXISTS for CREATE TABLE,
    /// so we wrap with an OBJECT_ID guard. Postgres / SQLite use the
    /// native form.
    fn create_table_if_not_exists(&self, table_qualified: &str, cols: &str) -> String {
        format!(
            "CREATE TABLE IF NOT EXISTS {} ({})",
            table_qualified, cols
        )
    }

    /// SQL fragment that returns the most recently inserted identity value
    /// on the current session. Always a single-column / single-row scalar.
    fn last_insert_id_sql(&self) -> &'static str;

    /// Concatenation operator used in expressions: `+` on MSSQL, `||` elsewhere.
    fn concat_op(&self) -> &'static str;

    /// Bind-parameter placeholder for the `idx`-th parameter (1-based).
    /// MSSQL & SQLite use `?`; Postgres uses `$N`.
    fn param_marker(&self, idx: usize) -> String;

    /// Render `DROP INDEX <name> [ON <table>]`. MSSQL requires the
    /// `ON table` clause; Postgres and SQLite reject it.
    fn drop_index_sql(&self, index_qualified: &str, table_qualified: &str) -> String {
        // Default — no `ON table`. MSSQL overrides.
        let _ = table_qualified;
        format!("DROP INDEX {}", index_qualified)
    }

    /// Backend-flavored `BEGIN TRANSACTION`. Used by op 19 (Begin Transaction).
    fn begin_txn(&self) -> &'static str {
        "BEGIN TRANSACTION"
    }

    /// Backend-flavored `COMMIT`.
    fn commit(&self) -> &'static str {
        "COMMIT"
    }

    /// Backend-flavored `ROLLBACK`.
    fn rollback(&self) -> &'static str {
        "ROLLBACK"
    }
}

pub struct MssqlDialect;
pub struct PostgresDialect;
pub struct SqliteDialect;

impl Dialect for MssqlDialect {
    fn limit_kind(&self) -> LimitKind {
        LimitKind::SelectTop
    }
    fn quote_ident(&self, name: &str) -> String {
        // MSSQL: bracket-quoted with escaped `]`.
        format!("[{}]", name.replace(']', "]]"))
    }
    fn identity_column(&self) -> &'static str {
        "INT IDENTITY(1,1) PRIMARY KEY"
    }
    fn last_insert_id_sql(&self) -> &'static str {
        "SELECT CAST(SCOPE_IDENTITY() AS BIGINT)"
    }
    fn concat_op(&self) -> &'static str {
        "+"
    }
    fn param_marker(&self, _idx: usize) -> String {
        "?".to_string()
    }
    fn drop_index_sql(&self, index_qualified: &str, table_qualified: &str) -> String {
        format!("DROP INDEX {} ON {}", index_qualified, table_qualified)
    }
    fn create_table_if_not_exists(&self, table_qualified: &str, cols: &str) -> String {
        // MSSQL: guard via OBJECT_ID — works with both schema-qualified and
        // bare names; sys.tables alone wouldn't catch cross-schema cases.
        format!(
            "IF OBJECT_ID(N'{}', N'U') IS NULL CREATE TABLE {} ({})",
            table_qualified.replace('\'', "''"),
            table_qualified,
            cols
        )
    }
}

impl Dialect for PostgresDialect {
    fn limit_kind(&self) -> LimitKind {
        LimitKind::LimitSuffix
    }
    fn quote_ident(&self, name: &str) -> String {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
    fn identity_column(&self) -> &'static str {
        "BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY"
    }
    fn last_insert_id_sql(&self) -> &'static str {
        // Caller always pairs this with a preceding INSERT ... RETURNING
        // strategy in the Postgres backend; this is the fallback.
        "SELECT lastval()"
    }
    fn concat_op(&self) -> &'static str {
        "||"
    }
    fn param_marker(&self, idx: usize) -> String {
        format!("${idx}")
    }
}

impl Dialect for SqliteDialect {
    fn limit_kind(&self) -> LimitKind {
        LimitKind::LimitSuffix
    }
    fn quote_ident(&self, name: &str) -> String {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
    fn identity_column(&self) -> &'static str {
        "INTEGER PRIMARY KEY AUTOINCREMENT"
    }
    fn last_insert_id_sql(&self) -> &'static str {
        "SELECT last_insert_rowid()"
    }
    fn concat_op(&self) -> &'static str {
        "||"
    }
    fn param_marker(&self, _idx: usize) -> String {
        "?".to_string()
    }
    fn begin_txn(&self) -> &'static str {
        // SQLite ignores the BEGIN TRANSACTION wording but accepts it.
        "BEGIN"
    }
}

/// Return a `&dyn Dialect` for the currently configured backend.
///
/// Reads `state().backend` under the state mutex. Cheap — the per-backend
/// dialect structs are zero-sized.
pub fn active() -> &'static dyn Dialect {
    let backend = state()
        .lock()
        .map(|st| st.backend)
        .unwrap_or(Backend::Mssql);
    for_backend(backend)
}

pub fn for_backend(backend: Backend) -> &'static dyn Dialect {
    match backend {
        Backend::Mssql => &MssqlDialect,
        Backend::Postgres => &PostgresDialect,
        Backend::Sqlite => &SqliteDialect,
    }
}

/// Render a `SELECT [TOP n] cols FROM ... [WHERE ...] [ORDER BY ...] [LIMIT n]`
/// statement, picking TOP-vs-LIMIT based on dialect.
///
/// `where_clause` and `order_by` are passed verbatim (without keyword), or
/// empty for none.
pub fn select_with_limit(
    dialect: &dyn Dialect,
    n: u32,
    cols: &str,
    from: &str,
    where_clause: &str,
    order_by: &str,
) -> String {
    let mut s = String::from("SELECT ");
    if matches!(dialect.limit_kind(), LimitKind::SelectTop) {
        s.push_str(&format!("TOP {n} "));
    }
    s.push_str(cols);
    s.push_str(" FROM ");
    s.push_str(from);
    if !where_clause.is_empty() {
        s.push_str(" WHERE ");
        s.push_str(where_clause);
    }
    if !order_by.is_empty() {
        s.push_str(" ORDER BY ");
        s.push_str(order_by);
    }
    if matches!(dialect.limit_kind(), LimitKind::LimitSuffix) {
        s.push_str(&format!(" LIMIT {n}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mssql_uses_top() {
        let s = select_with_limit(&MssqlDialect, 1, "*", "[t]", "a = 1", "a ASC");
        assert!(s.starts_with("SELECT TOP 1 "));
        assert!(!s.contains("LIMIT"));
    }

    #[test]
    fn postgres_uses_limit_suffix() {
        let s = select_with_limit(&PostgresDialect, 5, "*", "\"t\"", "", "id");
        assert!(s.starts_with("SELECT *"));
        assert!(s.ends_with(" LIMIT 5"));
    }

    #[test]
    fn sqlite_uses_limit_suffix() {
        let s = select_with_limit(&SqliteDialect, 10, "id", "\"t\"", "id > 0", "");
        assert!(s.contains("WHERE id > 0"));
        assert!(s.ends_with(" LIMIT 10"));
    }

    #[test]
    fn quote_ident_per_backend() {
        assert_eq!(MssqlDialect.quote_ident("foo"), "[foo]");
        assert_eq!(PostgresDialect.quote_ident("foo"), "\"foo\"");
        assert_eq!(SqliteDialect.quote_ident("foo"), "\"foo\"");
        // Escaping
        assert_eq!(MssqlDialect.quote_ident("a]b"), "[a]]b]");
        assert_eq!(PostgresDialect.quote_ident("a\"b"), "\"a\"\"b\"");
    }
}
