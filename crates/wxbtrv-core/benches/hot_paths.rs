//! Criterion benches for the hot paths in wxbtrv-core.
//!
//! Run with: `cargo bench -p wxbtrv-core`
//! Results land under `target/criterion/`.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use wxbtrv_core::ops::sql_helpers::{
    build_continuation_where, build_key_where, build_order_by_cols, col_to_sql_literal,
    parse_filter_terms,
};
use wxbtrv_core::record::{pack_row, unpack_row};
use wxbtrv_core::state::{IntField, RuntimeIndex, TableMeta};

// ── Fixture builders ────────────────────────────────────────────────────────

fn make_field(num: u32, name: &str, native_type: i32, length: u32, offset: u32) -> IntField {
    IntField {
        num,
        name: name.into(),
        native_type,
        length,
        offset,
        field_index: None,
        default_value: None,
    }
}

/// Build a TEST_CUST-like 10-field synthetic table meta with 200-byte records.
fn cust_meta() -> TableMeta {
    let fields = vec![
        make_field(1, "ID", 1, 4, 0),
        make_field(2, "NAME", 0, 30, 4),
        make_field(3, "ADDR1", 0, 30, 34),
        make_field(4, "ADDR2", 0, 30, 64),
        make_field(5, "CITY", 0, 20, 94),
        make_field(6, "STATE", 0, 2, 114),
        make_field(7, "ZIP", 0, 10, 116),
        make_field(8, "BAL", 5, 8, 126), // DECIMAL (BCD-packed)
        make_field(9, "STATUS", 1, 2, 134),
        make_field(10, "NOTE", 0, 64, 136),
    ];
    TableMeta {
        table_name: "TEST_CUST".into(),
        schema_name: "dbo".into(),
        db_name: "WXTEST".into(),
        record_length: 200,
        page_size: 4096,
        file_flags: 0,
        fields,
        indexes: vec![RuntimeIndex {
            num: 1,
            field_nums: vec![1],
            attrs: vec![0],
            desc: vec![false],
            null_values: vec![0],
            key_len: 4,
        }],
        recnum_col: "MDS_RECNUM".into(),
        ignore_null_values: true,
        trim_string_fields: true,
        translate_oem_to_ansi: false,
        primary_index: None,
        local_cache: false,
    }
}

fn cust_row_values() -> Vec<String> {
    vec![
        "12345".into(),
        "ACME WIDGETS INC".into(),
        "123 MAIN ST".into(),
        "SUITE 400".into(),
        "SPRINGFIELD".into(),
        "IL".into(),
        "62701".into(),
        "1234.56".into(),
        "1".into(),
        "PREFERRED CUSTOMER".into(),
    ]
}

/// Build a synthetic filter descriptor with 3 terms targeting fields 1,9,2 of
/// `cust_meta()`. Layout matches what `parse_filter_terms` expects.
fn three_term_filter() -> Vec<u8> {
    // 8-byte pre-GNE header (caller passes start=8).
    let mut buf = vec![0u8; 8];
    // Term 1: INT field at offset 0 (ID), len=4, cmp=GE(2), connector=AND(1)
    buf.push(1);
    buf.extend_from_slice(&4u16.to_le_bytes());
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf.push(2);
    buf.push(1);
    buf.extend_from_slice(&100i32.to_le_bytes());
    // Term 2: INT field at offset 134 (STATUS), len=2, cmp=EQ(0), connector=OR(2)
    buf.push(1);
    buf.extend_from_slice(&2u16.to_le_bytes());
    buf.extend_from_slice(&134u16.to_le_bytes());
    buf.push(0);
    buf.push(2);
    buf.extend_from_slice(&1i16.to_le_bytes());
    // Term 3: STRING field at offset 114 (STATE), len=2, cmp=EQ(0), connector=end(0)
    buf.push(0);
    buf.extend_from_slice(&2u16.to_le_bytes());
    buf.extend_from_slice(&114u16.to_le_bytes());
    buf.push(0);
    buf.push(0);
    buf.extend_from_slice(b"IL");
    buf
}

// ── Benches ─────────────────────────────────────────────────────────────────

fn bench_build_key_where(c: &mut Criterion) {
    let cols: Vec<(String, String)> = vec![
        ("[A]".into(), "'foo'".into()),
        ("[B]".into(), "5".into()),
        ("[C]".into(), "'bar'".into()),
    ];
    c.bench_function("build_key_where_3seg_gt", |b| {
        b.iter(|| build_key_where(black_box(&cols), black_box(">")))
    });
}

fn bench_build_continuation_where(c: &mut Criterion) {
    let cols: Vec<(String, String, bool)> = vec![
        ("[A]".into(), "'foo'".into(), false),
        ("[B]".into(), "5".into(), false),
        ("[C]".into(), "'bar'".into(), false),
    ];
    c.bench_function("build_continuation_where_3seg_fwd", |b| {
        b.iter(|| {
            build_continuation_where(
                black_box(&cols),
                black_box(1),
                black_box(987654),
                black_box("[MDS_RECNUM]"),
            )
        })
    });
}

fn bench_build_order_by_cols(c: &mut Criterion) {
    let refs: Vec<(String, bool)> = vec![
        ("[A]".into(), false),
        ("[B]".into(), false),
        ("[C]".into(), false),
        ("[D]".into(), true),
        ("[E]".into(), false),
    ];
    c.bench_function("build_order_by_cols_5seg_tiebreak", |b| {
        b.iter(|| {
            build_order_by_cols(
                black_box(&refs),
                black_box(1),
                black_box(true),
                black_box("[MDS_RECNUM]"),
            )
        })
    });
}

fn bench_col_to_sql_literal(c: &mut Criterion) {
    let str_field = make_field(1, "NAME", 0, 32, 0);
    let int_field = make_field(2, "ID", 1, 4, 0);
    let dec_field = make_field(3, "BAL", 5, 8, 0);
    let mut group = c.benchmark_group("col_to_sql_literal");
    group.bench_function("string_with_quotes", |b| {
        b.iter(|| col_to_sql_literal(black_box(&str_field), black_box("o'brien & co  ")))
    });
    group.bench_function("int", |b| {
        b.iter(|| col_to_sql_literal(black_box(&int_field), black_box(" 42 ")))
    });
    group.bench_function("decimal", |b| {
        b.iter(|| col_to_sql_literal(black_box(&dec_field), black_box("1234.56")))
    });
    group.finish();
}

fn bench_parse_filter_terms(c: &mut Criterion) {
    let meta = cust_meta();
    let buf = three_term_filter();
    c.bench_function("parse_filter_terms_3term", |b| {
        b.iter(|| {
            let r = parse_filter_terms(black_box(&meta), black_box(&buf), 8, 3);
            black_box(r);
        })
    });
}

fn bench_pack_row(c: &mut Criterion) {
    let meta = cust_meta();
    let vals = cust_row_values();
    c.bench_function("pack_row_10field_200b", |b| {
        b.iter(|| {
            let p = pack_row(
                black_box(&meta.fields),
                black_box(&vals),
                black_box(meta.record_length),
            );
            black_box(p);
        })
    });
}

fn bench_unpack_row(c: &mut Criterion) {
    let meta = cust_meta();
    let vals = cust_row_values();
    let packed = pack_row(&meta.fields, &vals, meta.record_length);
    c.bench_function("unpack_row_10field_200b", |b| {
        b.iter(|| {
            let r = unpack_row(black_box(&meta.fields), black_box(&packed));
            black_box(r);
        })
    });
}

criterion_group!(
    benches,
    bench_build_key_where,
    bench_build_continuation_where,
    bench_build_order_by_cols,
    bench_col_to_sql_literal,
    bench_parse_filter_terms,
    bench_pack_row,
    bench_unpack_row,
);
criterion_main!(benches);
