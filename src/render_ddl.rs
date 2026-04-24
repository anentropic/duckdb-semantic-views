//! DDL reconstruction: renders `CREATE OR REPLACE SEMANTIC VIEW` DDL
//! from a stored `SemanticViewDefinition`.
//!
//! The DDL reconstruction follows the body parser clause ordering:
//! TABLES -> RELATIONSHIPS -> FACTS -> DIMENSIONS -> METRICS
//! with optional clauses omitted when empty.
//!
//! This module is always compiled (not feature-gated) so that unit tests
//! can run under `cargo test` without the `extension` feature.

use crate::model::{AccessModifier, NullsOrder, SemanticViewDefinition, SortOrder};

/// SQL single-quote escaping: `'` -> `''`.
fn escape_single_quote(s: &str) -> String {
    s.replace('\'', "''")
}

/// Append ` COMMENT = '<escaped>'` to `out` if comment is present.
fn emit_comment(out: &mut String, comment: Option<&String>) {
    if let Some(c) = comment {
        out.push_str(" COMMENT = '");
        out.push_str(&escape_single_quote(c));
        out.push('\'');
    }
}

/// Append ` WITH SYNONYMS = ('<escaped1>', '<escaped2>')` to `out` if non-empty.
fn emit_synonyms(out: &mut String, synonyms: &[String]) {
    if !synonyms.is_empty() {
        out.push_str(" WITH SYNONYMS = (");
        for (i, s) in synonyms.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push('\'');
            out.push_str(&escape_single_quote(s));
            out.push('\'');
        }
        out.push(')');
    }
}

/// Emit TABLES clause entries.
fn emit_tables(out: &mut String, def: &SemanticViewDefinition) {
    out.push_str("TABLES (\n");
    for (i, table) in def.tables.iter().enumerate() {
        out.push_str("    ");
        out.push_str(&table.alias);
        out.push_str(" AS ");
        out.push_str(&table.table);
        if !table.pk_columns.is_empty() {
            out.push_str(" PRIMARY KEY (");
            out.push_str(&table.pk_columns.join(", "));
            out.push(')');
        }
        for uc in &table.unique_constraints {
            out.push_str(" UNIQUE (");
            out.push_str(&uc.join(", "));
            out.push(')');
        }
        emit_comment(out, table.comment.as_ref());
        emit_synonyms(out, &table.synonyms);
        if i + 1 < def.tables.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str(")\n");
}

/// Emit RELATIONSHIPS clause entries.
fn emit_relationships(out: &mut String, def: &SemanticViewDefinition) {
    out.push_str("RELATIONSHIPS (\n");
    for (i, join) in def.joins.iter().enumerate() {
        out.push_str("    ");
        if let Some(ref rel_name) = join.name {
            out.push_str(rel_name);
        }
        out.push_str(" AS ");
        out.push_str(&join.from_alias);
        out.push('(');
        out.push_str(&join.fk_columns.join(", "));
        out.push_str(") REFERENCES ");
        out.push_str(&join.table);
        if i + 1 < def.joins.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str(")\n");
}

/// Emit FACTS clause entries.
fn emit_facts(out: &mut String, def: &SemanticViewDefinition) {
    out.push_str("FACTS (\n");
    for (i, fact) in def.facts.iter().enumerate() {
        out.push_str("    ");
        if matches!(fact.access, AccessModifier::Private) {
            out.push_str("PRIVATE ");
        }
        if let Some(ref src) = fact.source_table {
            out.push_str(src);
            out.push('.');
        }
        out.push_str(&fact.name);
        out.push_str(" AS ");
        out.push_str(&fact.expr);
        emit_comment(out, fact.comment.as_ref());
        emit_synonyms(out, &fact.synonyms);
        if i + 1 < def.facts.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str(")\n");
}

/// Emit DIMENSIONS clause entries.
fn emit_dimensions(out: &mut String, def: &SemanticViewDefinition) {
    out.push_str("DIMENSIONS (\n");
    for (i, dim) in def.dimensions.iter().enumerate() {
        out.push_str("    ");
        if let Some(ref src) = dim.source_table {
            out.push_str(src);
            out.push('.');
        }
        out.push_str(&dim.name);
        out.push_str(" AS ");
        out.push_str(&dim.expr);
        emit_comment(out, dim.comment.as_ref());
        emit_synonyms(out, &dim.synonyms);
        if i + 1 < def.dimensions.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str(")\n");
}

/// Emit a window metric expression reconstructed from its parsed `WindowSpec`.
///
/// Format: `FUNC(inner_metric[, extra_args]) OVER (PARTITION BY EXCLUDING d1, d2 [ORDER BY ...] [frame])`
fn emit_window_expr(out: &mut String, ws: &crate::model::WindowSpec) {
    // Function call: e.g., AVG(total_qty) or LAG(total_qty, 30)
    out.push_str(&ws.window_function);
    out.push('(');
    out.push_str(&ws.inner_metric);
    for arg in &ws.extra_args {
        out.push_str(", ");
        out.push_str(arg);
    }
    out.push(')');

    // OVER clause
    out.push_str(" OVER (");
    let has_partition = if !ws.excluding_dims.is_empty() {
        out.push_str("PARTITION BY EXCLUDING ");
        out.push_str(&ws.excluding_dims.join(", "));
        true
    } else if !ws.partition_dims.is_empty() {
        out.push_str("PARTITION BY ");
        out.push_str(&ws.partition_dims.join(", "));
        true
    } else {
        false
    };
    if !ws.order_by.is_empty() {
        if has_partition {
            out.push(' ');
        }
        out.push_str("ORDER BY ");
        for (j, ob) in ws.order_by.iter().enumerate() {
            if j > 0 {
                out.push_str(", ");
            }
            out.push_str(&ob.expr);
            match ob.order {
                SortOrder::Asc => {} // default, omit
                SortOrder::Desc => out.push_str(" DESC"),
            }
            // Always emit explicit NULLS to avoid DuckDB version divergence
            match ob.nulls {
                NullsOrder::Last => out.push_str(" NULLS LAST"),
                NullsOrder::First => out.push_str(" NULLS FIRST"),
            }
        }
    }
    if let Some(ref frame) = ws.frame_clause {
        if has_partition || !ws.order_by.is_empty() {
            out.push(' ');
        }
        out.push_str(frame);
    }
    out.push(')');
}

/// Emit METRICS clause entries.
fn emit_metrics(out: &mut String, def: &SemanticViewDefinition) {
    out.push_str("METRICS (\n");
    for (i, metric) in def.metrics.iter().enumerate() {
        out.push_str("    ");
        if matches!(metric.access, AccessModifier::Private) {
            out.push_str("PRIVATE ");
        }
        if let Some(ref src) = metric.source_table {
            out.push_str(src);
            out.push('.');
        }
        out.push_str(&metric.name);
        if !metric.using_relationships.is_empty() {
            out.push_str(" USING (");
            out.push_str(&metric.using_relationships.join(", "));
            out.push(')');
        }
        if !metric.non_additive_by.is_empty() {
            out.push_str(" NON ADDITIVE BY (");
            for (j, na) in metric.non_additive_by.iter().enumerate() {
                if j > 0 {
                    out.push_str(", ");
                }
                out.push_str(&na.dimension);
                match na.order {
                    SortOrder::Asc => {} // default, omit
                    SortOrder::Desc => out.push_str(" DESC"),
                }
                // Always emit explicit NULLS to avoid DuckDB version divergence
                match na.nulls {
                    NullsOrder::Last => out.push_str(" NULLS LAST"),
                    NullsOrder::First => out.push_str(" NULLS FIRST"),
                }
            }
            out.push(')');
        }
        out.push_str(" AS ");
        if let Some(ref ws) = metric.window_spec {
            // Reconstruct the OVER clause from parsed WindowSpec for normalized formatting
            emit_window_expr(out, ws);
        } else {
            out.push_str(&metric.expr);
        }
        emit_comment(out, metric.comment.as_ref());
        emit_synonyms(out, &metric.synonyms);
        if i + 1 < def.metrics.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str(")\n");
}

/// Emit MATERIALIZATIONS clause entries.
fn emit_materializations(out: &mut String, def: &SemanticViewDefinition) {
    out.push_str("MATERIALIZATIONS (\n");
    for (i, mat) in def.materializations.iter().enumerate() {
        out.push_str("    ");
        out.push_str(&mat.name);
        out.push_str(" AS (\n");
        out.push_str("        TABLE ");
        out.push_str(&mat.table);
        if !mat.dimensions.is_empty() || !mat.metrics.is_empty() {
            out.push_str(",\n");
        } else {
            out.push('\n');
        }
        if !mat.dimensions.is_empty() {
            out.push_str("        DIMENSIONS (");
            out.push_str(&mat.dimensions.join(", "));
            out.push(')');
            if !mat.metrics.is_empty() {
                out.push(',');
            }
            out.push('\n');
        }
        if !mat.metrics.is_empty() {
            out.push_str("        METRICS (");
            out.push_str(&mat.metrics.join(", "));
            out.push_str(")\n");
        }
        out.push_str("    )");
        if i + 1 < def.materializations.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str(")\n");
}

/// Reconstruct a `CREATE OR REPLACE SEMANTIC VIEW` DDL statement from a stored
/// definition. Returns `Err` for legacy definitions (empty `tables` vec).
///
/// The output follows body parser clause ordering:
/// TABLES -> RELATIONSHIPS -> FACTS -> DIMENSIONS -> METRICS
pub fn render_create_ddl(name: &str, def: &SemanticViewDefinition) -> Result<String, String> {
    if def.tables.is_empty() {
        return Err(
            "Legacy definition format; please re-create using CREATE OR REPLACE SEMANTIC VIEW"
                .to_string(),
        );
    }

    let mut out = String::with_capacity(512);

    // Header
    out.push_str("CREATE OR REPLACE SEMANTIC VIEW ");
    out.push_str(name);

    // View-level comment
    emit_comment(&mut out, def.comment.as_ref());

    // AS keyword
    out.push_str(" AS\n");

    emit_tables(&mut out, def);

    if !def.joins.is_empty() {
        emit_relationships(&mut out, def);
    }
    if !def.facts.is_empty() {
        emit_facts(&mut out, def);
    }
    if !def.dimensions.is_empty() {
        emit_dimensions(&mut out, def);
    }
    if !def.metrics.is_empty() {
        emit_metrics(&mut out, def);
    }
    if !def.materializations.is_empty() {
        emit_materializations(&mut out, def);
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Dimension, Fact, Join, Metric, TableRef};

    fn minimal_def() -> SemanticViewDefinition {
        SemanticViewDefinition {
            tables: vec![TableRef {
                alias: "o".to_string(),
                table: "orders".to_string(),
                pk_columns: vec!["id".to_string()],
                ..Default::default()
            }],
            dimensions: vec![Dimension {
                name: "region".to_string(),
                expr: "o.region".to_string(),
                source_table: Some("o".to_string()),
                ..Default::default()
            }],
            metrics: vec![Metric {
                name: "revenue".to_string(),
                expr: "SUM(o.amount)".to_string(),
                source_table: Some("o".to_string()),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn test_minimal_view() {
        let def = minimal_def();
        let ddl = render_create_ddl("my_view", &def).unwrap();
        assert!(ddl.starts_with("CREATE OR REPLACE SEMANTIC VIEW my_view"));
        assert!(ddl.contains("TABLES ("));
        assert!(ddl.contains("DIMENSIONS ("));
        assert!(ddl.contains("METRICS ("));
        assert!(ddl.contains("o AS orders PRIMARY KEY (id)"));
        assert!(ddl.contains("o.region AS o.region"));
        assert!(ddl.contains("o.revenue AS SUM(o.amount)"));
    }

    #[test]
    fn test_view_comment() {
        let mut def = minimal_def();
        def.comment = Some("My view comment".to_string());
        let ddl = render_create_ddl("cv", &def).unwrap();
        assert!(ddl.contains("COMMENT = 'My view comment'"));
        // COMMENT should come before AS
        let comment_pos = ddl.find("COMMENT = 'My view comment'").unwrap();
        let as_pos = ddl.find(" AS\n").unwrap();
        assert!(comment_pos < as_pos);
    }

    #[test]
    fn test_comment_with_single_quote() {
        let mut def = minimal_def();
        def.comment = Some("it's a test".to_string());
        let ddl = render_create_ddl("cv", &def).unwrap();
        assert!(ddl.contains("COMMENT = 'it''s a test'"));
    }

    #[test]
    fn test_relationships() {
        let mut def = minimal_def();
        def.tables.push(TableRef {
            alias: "c".to_string(),
            table: "customers".to_string(),
            pk_columns: vec!["id".to_string()],
            ..Default::default()
        });
        def.joins = vec![Join {
            name: Some("order_customer".to_string()),
            from_alias: "o".to_string(),
            fk_columns: vec!["customer_id".to_string()],
            table: "c".to_string(),
            ..Default::default()
        }];
        let ddl = render_create_ddl("rv", &def).unwrap();
        assert!(ddl.contains("RELATIONSHIPS ("));
        assert!(ddl.contains("order_customer AS o(customer_id) REFERENCES c"));
    }

    #[test]
    fn test_facts() {
        let mut def = minimal_def();
        def.facts = vec![Fact {
            name: "margin".to_string(),
            expr: "o.amount - o.cost".to_string(),
            source_table: Some("o".to_string()),
            ..Default::default()
        }];
        let ddl = render_create_ddl("fv", &def).unwrap();
        assert!(ddl.contains("FACTS ("));
        assert!(ddl.contains("o.margin AS o.amount - o.cost"));
    }

    #[test]
    fn test_private_metric() {
        let mut def = minimal_def();
        def.metrics[0].access = AccessModifier::Private;
        let ddl = render_create_ddl("pv", &def).unwrap();
        assert!(ddl.contains("PRIVATE o.revenue AS SUM(o.amount)"));
    }

    #[test]
    fn test_using_relationships() {
        let mut def = minimal_def();
        def.metrics[0].using_relationships = vec!["rel1".to_string()];
        let ddl = render_create_ddl("uv", &def).unwrap();
        assert!(ddl.contains("USING (rel1)"));
        // USING should come before AS in the metric line
        let using_pos = ddl.find("USING (rel1)").unwrap();
        let as_pos = ddl[using_pos..].find(" AS ").unwrap() + using_pos;
        assert!(using_pos < as_pos);
    }

    #[test]
    fn test_synonyms() {
        let mut def = minimal_def();
        def.dimensions[0].synonyms = vec!["syn1".to_string(), "syn2".to_string()];
        let ddl = render_create_ddl("sv", &def).unwrap();
        assert!(ddl.contains("WITH SYNONYMS = ('syn1', 'syn2')"));
    }

    #[test]
    fn test_object_comments() {
        let mut def = minimal_def();
        def.tables[0].comment = Some("table comment".to_string());
        def.dimensions[0].comment = Some("dim comment".to_string());
        def.metrics[0].comment = Some("metric comment".to_string());

        let mut def2 = def.clone();
        def2.facts = vec![Fact {
            name: "f1".to_string(),
            expr: "o.x".to_string(),
            source_table: Some("o".to_string()),
            comment: Some("fact comment".to_string()),
            ..Default::default()
        }];

        let ddl = render_create_ddl("oc", &def).unwrap();
        assert!(ddl.contains("COMMENT = 'table comment'"));
        assert!(ddl.contains("COMMENT = 'dim comment'"));
        assert!(ddl.contains("COMMENT = 'metric comment'"));

        let ddl2 = render_create_ddl("oc2", &def2).unwrap();
        assert!(ddl2.contains("COMMENT = 'fact comment'"));
    }

    #[test]
    fn test_unique_constraints() {
        let mut def = minimal_def();
        def.tables[0].unique_constraints = vec![vec!["col1".to_string(), "col2".to_string()]];
        let ddl = render_create_ddl("uc", &def).unwrap();
        assert!(ddl.contains("UNIQUE (col1, col2)"));
    }

    #[test]
    fn test_empty_tables_error() {
        let def = SemanticViewDefinition {
            ..Default::default()
        };
        let result = render_create_ddl("legacy", &def);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Legacy definition format"));
    }

    #[test]
    fn test_omits_relationships_when_empty() {
        let def = minimal_def();
        let ddl = render_create_ddl("norels", &def).unwrap();
        assert!(!ddl.contains("RELATIONSHIPS"));
    }

    #[test]
    fn test_omits_facts_when_empty() {
        let def = minimal_def();
        let ddl = render_create_ddl("nofacts", &def).unwrap();
        assert!(!ddl.contains("FACTS"));
    }

    #[test]
    fn test_emit_comment_helper() {
        let mut out = String::new();
        emit_comment(&mut out, None);
        assert!(out.is_empty());

        let hello = "hello".to_string();
        emit_comment(&mut out, Some(&hello));
        assert_eq!(out, " COMMENT = 'hello'");
    }

    #[test]
    fn test_emit_synonyms_helper() {
        let mut out = String::new();
        emit_synonyms(&mut out, &[]);
        assert!(out.is_empty());

        emit_synonyms(&mut out, &["s1".to_string(), "s2".to_string()]);
        assert_eq!(out, " WITH SYNONYMS = ('s1', 's2')");
    }

    #[test]
    fn test_private_fact() {
        let mut def = minimal_def();
        def.facts = vec![Fact {
            name: "margin".to_string(),
            expr: "o.amount - o.cost".to_string(),
            source_table: Some("o".to_string()),
            access: AccessModifier::Private,
            ..Default::default()
        }];
        let ddl = render_create_ddl("pf", &def).unwrap();
        assert!(ddl.contains("PRIVATE o.margin AS o.amount - o.cost"));
    }

    #[test]
    fn test_derived_metric_no_source_table() {
        let mut def = minimal_def();
        def.metrics.push(Metric {
            name: "profit_margin".to_string(),
            expr: "revenue / cost".to_string(),
            source_table: None,
            ..Default::default()
        });
        let ddl = render_create_ddl("dm", &def).unwrap();
        // Derived metric should appear without source table prefix
        assert!(ddl.contains("    profit_margin AS revenue / cost"));
    }

    #[test]
    fn test_multiple_using_relationships() {
        let mut def = minimal_def();
        def.metrics[0].using_relationships = vec!["rel_a".to_string(), "rel_b".to_string()];
        let ddl = render_create_ddl("mu", &def).unwrap();
        assert!(ddl.contains("USING (rel_a, rel_b)"));
    }

    #[test]
    fn test_synonym_with_single_quote() {
        let mut def = minimal_def();
        def.dimensions[0].synonyms = vec!["it's syn".to_string()];
        let ddl = render_create_ddl("sq", &def).unwrap();
        assert!(ddl.contains("WITH SYNONYMS = ('it''s syn')"));
    }

    #[test]
    fn test_clause_ordering() {
        let mut def = minimal_def();
        def.tables.push(TableRef {
            alias: "c".to_string(),
            table: "customers".to_string(),
            pk_columns: vec!["id".to_string()],
            ..Default::default()
        });
        def.joins = vec![Join {
            name: Some("r1".to_string()),
            from_alias: "o".to_string(),
            fk_columns: vec!["cid".to_string()],
            table: "c".to_string(),
            ..Default::default()
        }];
        def.facts = vec![Fact {
            name: "f1".to_string(),
            expr: "o.x".to_string(),
            source_table: Some("o".to_string()),
            ..Default::default()
        }];

        let ddl = render_create_ddl("ordered", &def).unwrap();
        let tables_pos = ddl.find("TABLES (").unwrap();
        let rels_pos = ddl.find("RELATIONSHIPS (").unwrap();
        let facts_pos = ddl.find("FACTS (").unwrap();
        let dims_pos = ddl.find("DIMENSIONS (").unwrap();
        let metrics_pos = ddl.find("METRICS (").unwrap();

        assert!(tables_pos < rels_pos);
        assert!(rels_pos < facts_pos);
        assert!(facts_pos < dims_pos);
        assert!(dims_pos < metrics_pos);
    }

    #[test]
    fn test_non_additive_by_single_dim_default_order() {
        use crate::model::{NonAdditiveDim, NullsOrder, SortOrder};
        let mut def = minimal_def();
        def.metrics[0].non_additive_by = vec![NonAdditiveDim {
            dimension: "date_dim".to_string(),
            order: SortOrder::Asc,
            nulls: NullsOrder::Last,
        }];
        let ddl = render_create_ddl("nav", &def).unwrap();
        assert!(
            ddl.contains("NON ADDITIVE BY (date_dim NULLS LAST)"),
            "Expected NON ADDITIVE BY clause: {ddl}"
        );
        // NON ADDITIVE BY should come before AS
        let na_pos = ddl.find("NON ADDITIVE BY").unwrap();
        let as_pos = ddl[na_pos..].find(" AS ").unwrap() + na_pos;
        assert!(na_pos < as_pos);
    }

    #[test]
    fn test_non_additive_by_desc_nulls_first() {
        use crate::model::{NonAdditiveDim, NullsOrder, SortOrder};
        let mut def = minimal_def();
        def.metrics[0].non_additive_by = vec![NonAdditiveDim {
            dimension: "date_dim".to_string(),
            order: SortOrder::Desc,
            nulls: NullsOrder::First,
        }];
        let ddl = render_create_ddl("nav2", &def).unwrap();
        assert!(
            ddl.contains("NON ADDITIVE BY (date_dim DESC NULLS FIRST)"),
            "Expected NON ADDITIVE BY with DESC NULLS FIRST: {ddl}"
        );
    }

    #[test]
    fn test_non_additive_by_roundtrip() {
        use crate::body_parser::parse_keyword_body;
        use crate::model::{NonAdditiveDim, NullsOrder, SortOrder};
        let mut def = minimal_def();
        def.dimensions.push(Dimension {
            name: "snapshot_date".to_string(),
            expr: "o.snapshot_date".to_string(),
            source_table: Some("o".to_string()),
            ..Default::default()
        });
        def.metrics[0].non_additive_by = vec![NonAdditiveDim {
            dimension: "snapshot_date".to_string(),
            order: SortOrder::Desc,
            nulls: NullsOrder::First,
        }];
        let ddl1 = render_create_ddl("rt", &def).unwrap();
        // Parse the generated DDL body (everything after "AS\n")
        let as_pos = ddl1.find(" AS\n").unwrap();
        let body = format!("AS {}", &ddl1[as_pos + 4..]);
        let kb = parse_keyword_body(&body, 0).expect("Round-trip parse should succeed");
        // Reconstruct from parsed body
        let def2 = SemanticViewDefinition {
            tables: kb.tables,
            dimensions: kb.dimensions,
            metrics: kb.metrics,
            joins: kb.relationships,
            facts: kb.facts,
            ..Default::default()
        };
        let ddl2 = render_create_ddl("rt", &def2).unwrap();
        assert_eq!(ddl1, ddl2, "Round-trip DDL should be identical");
    }

    #[test]
    fn test_non_additive_by_with_using() {
        use crate::model::{NonAdditiveDim, NullsOrder, SortOrder};
        let mut def = minimal_def();
        def.metrics[0].using_relationships = vec!["rel1".to_string()];
        def.metrics[0].non_additive_by = vec![NonAdditiveDim {
            dimension: "region".to_string(),
            order: SortOrder::Desc,
            nulls: NullsOrder::First,
        }];
        let ddl = render_create_ddl("uv", &def).unwrap();
        // USING should come before NON ADDITIVE BY
        let using_pos = ddl.find("USING (rel1)").unwrap();
        let na_pos = ddl.find("NON ADDITIVE BY").unwrap();
        let as_pos = ddl[na_pos..].find(" AS ").unwrap() + na_pos;
        assert!(using_pos < na_pos, "USING should precede NON ADDITIVE BY");
        assert!(na_pos < as_pos, "NON ADDITIVE BY should precede AS");
    }

    #[test]
    fn test_multi_column_fk() {
        let mut def = minimal_def();
        def.tables.push(TableRef {
            alias: "c".to_string(),
            table: "customers".to_string(),
            pk_columns: vec!["id".to_string(), "region".to_string()],
            ..Default::default()
        });
        def.joins = vec![Join {
            name: Some("order_cust".to_string()),
            from_alias: "o".to_string(),
            fk_columns: vec!["cust_id".to_string(), "cust_region".to_string()],
            table: "c".to_string(),
            ..Default::default()
        }];
        let ddl = render_create_ddl("mfk", &def).unwrap();
        assert!(ddl.contains("order_cust AS o(cust_id, cust_region) REFERENCES c"));
    }

    #[test]
    fn test_window_spec_basic_emission() {
        use crate::model::{NullsOrder, SortOrder, WindowOrderBy, WindowSpec};
        let mut def = minimal_def();
        def.metrics.push(Metric {
            name: "avg_qty".to_string(),
            expr: "AVG(total_qty) OVER (PARTITION BY EXCLUDING region ORDER BY month)".to_string(),
            source_table: Some("o".to_string()),
            window_spec: Some(WindowSpec {
                window_function: "AVG".to_string(),
                inner_metric: "total_qty".to_string(),
                extra_args: vec![],
                excluding_dims: vec!["region".to_string()],
                partition_dims: vec![],
                order_by: vec![WindowOrderBy {
                    expr: "month".to_string(),
                    order: SortOrder::Asc,
                    nulls: NullsOrder::Last,
                }],
                frame_clause: None,
            }),
            ..Default::default()
        });
        let ddl = render_create_ddl("wv", &def).unwrap();
        assert!(
            ddl.contains(
                "AVG(total_qty) OVER (PARTITION BY EXCLUDING region ORDER BY month NULLS LAST)"
            ),
            "Expected window OVER clause in DDL: {ddl}"
        );
    }

    #[test]
    fn test_window_spec_with_frame_clause() {
        use crate::model::{NullsOrder, SortOrder, WindowOrderBy, WindowSpec};
        let mut def = minimal_def();
        def.metrics.push(Metric {
            name: "avg_qty_7d".to_string(),
            expr: "AVG(total_qty) OVER (...)".to_string(),
            source_table: Some("o".to_string()),
            window_spec: Some(WindowSpec {
                window_function: "AVG".to_string(),
                inner_metric: "total_qty".to_string(),
                extra_args: vec![],
                excluding_dims: vec!["region".to_string()],
                partition_dims: vec![],
                order_by: vec![WindowOrderBy {
                    expr: "month".to_string(),
                    order: SortOrder::Desc,
                    nulls: NullsOrder::First,
                }],
                frame_clause: Some(
                    "RANGE BETWEEN INTERVAL '6 days' PRECEDING AND CURRENT ROW".to_string(),
                ),
            }),
            ..Default::default()
        });
        let ddl = render_create_ddl("wv2", &def).unwrap();
        assert!(
            ddl.contains("AVG(total_qty) OVER (PARTITION BY EXCLUDING region ORDER BY month DESC NULLS FIRST RANGE BETWEEN INTERVAL '6 days' PRECEDING AND CURRENT ROW)"),
            "Expected window OVER clause with frame in DDL: {ddl}"
        );
    }

    #[test]
    fn test_window_spec_roundtrip() {
        use crate::body_parser::parse_keyword_body;
        use crate::model::{NullsOrder, SortOrder, WindowOrderBy, WindowSpec};
        let mut def = minimal_def();
        // Add a base metric that the window metric references
        def.metrics.push(Metric {
            name: "total_qty".to_string(),
            expr: "SUM(o.qty)".to_string(),
            source_table: Some("o".to_string()),
            ..Default::default()
        });
        def.metrics.push(Metric {
            name: "avg_qty".to_string(),
            expr: "AVG(total_qty) OVER (PARTITION BY EXCLUDING region ORDER BY region)".to_string(),
            source_table: Some("o".to_string()),
            window_spec: Some(WindowSpec {
                window_function: "AVG".to_string(),
                inner_metric: "total_qty".to_string(),
                extra_args: vec![],
                excluding_dims: vec!["region".to_string()],
                partition_dims: vec![],
                order_by: vec![WindowOrderBy {
                    expr: "region".to_string(),
                    order: SortOrder::Asc,
                    nulls: NullsOrder::Last,
                }],
                frame_clause: None,
            }),
            ..Default::default()
        });
        let ddl1 = render_create_ddl("rt", &def).unwrap();
        // Parse the generated DDL body
        let as_pos = ddl1.find(" AS\n").unwrap();
        let body = format!("AS {}", &ddl1[as_pos + 4..]);
        let kb = parse_keyword_body(&body, 0).expect("Round-trip parse should succeed");
        let def2 = SemanticViewDefinition {
            tables: kb.tables,
            dimensions: kb.dimensions,
            metrics: kb.metrics,
            joins: kb.relationships,
            facts: kb.facts,
            ..Default::default()
        };
        let ddl2 = render_create_ddl("rt", &def2).unwrap();
        assert_eq!(ddl1, ddl2, "Round-trip DDL should be identical");
    }

    #[test]
    fn test_window_spec_partition_by_explicit() {
        use crate::body_parser::parse_keyword_body;
        use crate::model::{NullsOrder, SortOrder, WindowOrderBy, WindowSpec};

        let mut def = minimal_def();
        def.dimensions.push(Dimension {
            name: "month".to_string(),
            expr: "o.month".to_string(),
            source_table: Some("o".to_string()),
            ..Default::default()
        });
        def.metrics.push(Metric {
            name: "avg_rev".to_string(),
            expr: "AVG(revenue) OVER (PARTITION BY region ORDER BY month)".to_string(),
            source_table: Some("o".to_string()),
            window_spec: Some(WindowSpec {
                window_function: "AVG".to_string(),
                inner_metric: "revenue".to_string(),
                extra_args: vec![],
                excluding_dims: vec![],
                partition_dims: vec!["region".to_string()],
                order_by: vec![WindowOrderBy {
                    expr: "month".to_string(),
                    order: SortOrder::Asc,
                    nulls: NullsOrder::Last,
                }],
                frame_clause: None,
            }),
            ..Default::default()
        });
        let ddl = render_create_ddl("test", &def).unwrap();
        assert!(
            ddl.contains("PARTITION BY region"),
            "DDL should contain PARTITION BY region: {ddl}"
        );
        assert!(
            !ddl.contains("EXCLUDING"),
            "DDL should not contain EXCLUDING: {ddl}"
        );

        // Round-trip: parse the generated DDL and re-render
        let as_pos = ddl.find(" AS\n").unwrap();
        let body = format!("AS {}", &ddl[as_pos + 4..]);
        let kb = parse_keyword_body(&body, 0).expect("Round-trip parse should succeed");
        let ws = kb.metrics[1].window_spec.as_ref().unwrap();
        assert!(ws.excluding_dims.is_empty());
        assert_eq!(ws.partition_dims, vec!["region"]);
    }

    // -----------------------------------------------------------------------
    // Phase 54: MATERIALIZATIONS DDL reconstruction tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_materializations_emitted_when_present() {
        use crate::model::Materialization;
        let mut def = minimal_def();
        def.materializations = vec![Materialization {
            name: "daily_rev".to_string(),
            table: "daily_revenue_agg".to_string(),
            dimensions: vec!["region".to_string()],
            metrics: vec!["revenue".to_string()],
        }];
        let ddl = render_create_ddl("mv", &def).unwrap();
        assert!(
            ddl.contains("MATERIALIZATIONS ("),
            "DDL should contain MATERIALIZATIONS: {ddl}"
        );
        assert!(
            ddl.contains("daily_rev AS ("),
            "DDL should contain materialization name: {ddl}"
        );
        assert!(
            ddl.contains("TABLE daily_revenue_agg"),
            "DDL should contain TABLE sub-clause: {ddl}"
        );
        assert!(
            ddl.contains("DIMENSIONS (region)"),
            "DDL should contain DIMENSIONS sub-clause: {ddl}"
        );
        assert!(
            ddl.contains("METRICS (revenue)"),
            "DDL should contain METRICS sub-clause: {ddl}"
        );
    }

    #[test]
    fn test_materializations_omitted_when_empty() {
        let def = minimal_def();
        let ddl = render_create_ddl("nomats", &def).unwrap();
        assert!(
            !ddl.contains("MATERIALIZATIONS"),
            "DDL should not contain MATERIALIZATIONS when empty: {ddl}"
        );
    }

    #[test]
    fn test_materializations_clause_ordering() {
        use crate::model::Materialization;
        let mut def = minimal_def();
        def.materializations = vec![Materialization {
            name: "mat1".to_string(),
            table: "t1".to_string(),
            dimensions: vec!["region".to_string()],
            metrics: vec![],
        }];
        let ddl = render_create_ddl("ordered", &def).unwrap();
        let metrics_pos = ddl.find("METRICS (").unwrap();
        let mats_pos = ddl.find("MATERIALIZATIONS (").unwrap();
        assert!(
            metrics_pos < mats_pos,
            "MATERIALIZATIONS should come after METRICS"
        );
    }

    #[test]
    fn test_materializations_ddl_roundtrip() {
        use crate::body_parser::parse_keyword_body;
        use crate::model::Materialization;
        let mut def = minimal_def();
        def.materializations = vec![
            Materialization {
                name: "daily_rev".to_string(),
                table: "daily_revenue_agg".to_string(),
                dimensions: vec!["region".to_string()],
                metrics: vec!["revenue".to_string()],
            },
            Materialization {
                name: "monthly_rev".to_string(),
                table: "monthly_agg".to_string(),
                dimensions: vec![],
                metrics: vec!["revenue".to_string()],
            },
        ];
        let ddl1 = render_create_ddl("rt", &def).unwrap();
        // Parse the generated DDL body
        let as_pos = ddl1.find(" AS\n").unwrap();
        let body = format!("AS {}", &ddl1[as_pos + 4..]);
        let kb = parse_keyword_body(&body, 0).expect("Round-trip parse should succeed");
        let def2 = SemanticViewDefinition {
            tables: kb.tables,
            dimensions: kb.dimensions,
            metrics: kb.metrics,
            joins: kb.relationships,
            facts: kb.facts,
            materializations: kb.materializations,
            ..Default::default()
        };
        let ddl2 = render_create_ddl("rt", &def2).unwrap();
        assert_eq!(ddl1, ddl2, "Round-trip DDL should be identical");
    }
}
