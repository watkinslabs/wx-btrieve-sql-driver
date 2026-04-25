//! Backend-agnostic SQL bind parameters.
//!
//! Ops produce SQL with `dialect.param_marker(idx)` placeholders and a
//! parallel `Vec<SqlValue>` of bound values. Each backend marshals
//! `SqlValue` to its native parameter type:
//!
//!   - MSSQL via odbc-api: positional `?`, params bound through
//!     `odbc_api::parameter::InputParameter`.
//!   - Postgres: numbered `$1, $2, ...`, params passed as
//!     `&[&(dyn ToSql + Sync)]`.
//!   - SQLite via rusqlite: positional `?`, params via
//!     `rusqlite::params_from_iter`.
//!
//! The marshalling lives in `sql.rs`; this module just defines the value
//! type and a few convenience constructors.

use std::borrow::Cow;

#[derive(Clone, Debug, PartialEq)]
pub enum SqlValue {
    Null,
    Bool(bool),
    I32(i32),
    I64(i64),
    F64(f64),
    /// UTF-8 text. For Btrieve STRING/ZSTRING fields after trim/decode.
    Text(String),
    /// Raw bytes. For binary keys, BLOB columns, and pre-encoded date/time.
    Bytes(Vec<u8>),
}

impl SqlValue {
    pub fn null() -> Self {
        SqlValue::Null
    }
    pub fn text(s: impl Into<String>) -> Self {
        SqlValue::Text(s.into())
    }
    pub fn bytes(b: impl Into<Vec<u8>>) -> Self {
        SqlValue::Bytes(b.into())
    }
}

impl From<i32> for SqlValue {
    fn from(v: i32) -> Self {
        SqlValue::I32(v)
    }
}
impl From<i64> for SqlValue {
    fn from(v: i64) -> Self {
        SqlValue::I64(v)
    }
}
impl From<u32> for SqlValue {
    fn from(v: u32) -> Self {
        SqlValue::I64(v as i64)
    }
}
impl From<u16> for SqlValue {
    fn from(v: u16) -> Self {
        SqlValue::I32(v as i32)
    }
}
impl From<f64> for SqlValue {
    fn from(v: f64) -> Self {
        SqlValue::F64(v)
    }
}
impl From<bool> for SqlValue {
    fn from(v: bool) -> Self {
        SqlValue::Bool(v)
    }
}
impl From<String> for SqlValue {
    fn from(v: String) -> Self {
        SqlValue::Text(v)
    }
}
impl From<&str> for SqlValue {
    fn from(v: &str) -> Self {
        SqlValue::Text(v.to_string())
    }
}
impl From<Vec<u8>> for SqlValue {
    fn from(v: Vec<u8>) -> Self {
        SqlValue::Bytes(v)
    }
}
impl From<&[u8]> for SqlValue {
    fn from(v: &[u8]) -> Self {
        SqlValue::Bytes(v.to_vec())
    }
}
impl<T: Into<SqlValue>> From<Option<T>> for SqlValue {
    fn from(v: Option<T>) -> Self {
        match v {
            Some(x) => x.into(),
            None => SqlValue::Null,
        }
    }
}

/// Render `value` as a SQL literal for diagnostic logging only. Never used
/// to build executable SQL — that path always goes through bind params.
pub fn debug_literal(v: &SqlValue) -> Cow<'_, str> {
    match v {
        SqlValue::Null => Cow::Borrowed("NULL"),
        SqlValue::Bool(b) => Cow::Borrowed(if *b { "1" } else { "0" }),
        SqlValue::I32(i) => Cow::Owned(i.to_string()),
        SqlValue::I64(i) => Cow::Owned(i.to_string()),
        SqlValue::F64(f) => Cow::Owned(f.to_string()),
        SqlValue::Text(s) => Cow::Owned(format!("'{}'", s.replace('\'', "''"))),
        SqlValue::Bytes(b) => Cow::Owned(format!("0x{}", hex(b))),
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_conversions() {
        assert_eq!(SqlValue::from(1i32), SqlValue::I32(1));
        assert_eq!(SqlValue::from(1u32), SqlValue::I64(1));
        assert_eq!(SqlValue::from("hi"), SqlValue::Text("hi".into()));
        assert_eq!(SqlValue::from(None::<i32>), SqlValue::Null);
        assert_eq!(SqlValue::from(Some(5i32)), SqlValue::I32(5));
    }

    #[test]
    fn debug_literal_renders() {
        assert_eq!(debug_literal(&SqlValue::Null), "NULL");
        assert_eq!(debug_literal(&SqlValue::I64(42)), "42");
        assert_eq!(
            debug_literal(&SqlValue::Text("o'brien".into())),
            "'o''brien'"
        );
        assert_eq!(
            debug_literal(&SqlValue::Bytes(vec![0xde, 0xad])),
            "0xdead"
        );
    }
}
