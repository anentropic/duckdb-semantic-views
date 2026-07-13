//! Shared building blocks for the SELECT-emitting expansion strategies.
//!
//! §6.2 (code-review 2026-07-11) consolidates the four hand-rolled
//! SELECT-statement emitters (base, semi-additive, window, materialization)
//! onto shared pieces so their common shapes are defined once. This module is
//! grown incrementally, one piece per PR, under the sqllogictest / proptest /
//! differential oracle.
//!
//! First piece: [`SelectItem`], which owns the `[CAST(expr AS type)] AS alias`
//! rendering the emitters previously inlined at ~11 sites (the `if let
//! Some(type_str) = ...output_type { format!("CAST(...)") }` idiom). Callers
//! pass the already-resolved expression (scoped-alias rewrite / fact inlining
//! done) and the already-quoted output alias; the item owns only the optional
//! `output_type` CAST wrap and the trailing `AS alias`.
//!
//! Also here: the shared [`push_from_base`] (base-table FROM clause) and
//! [`push_group_by_ordinals`] (ordinal GROUP BY) emit helpers, each previously
//! copied across the base / CTE strategies.

use crate::model::SemanticViewDefinition;

use super::resolution::{qualify_and_quote_table_ref, quote_ident};

/// One `SELECT`-list item: an expression, an optional `output_type` CAST wrap,
/// and an output alias.
///
/// `expr` is the fully-resolved SQL expression (any scoped-alias rewrite or
/// fact/derived-metric inlining is the caller's responsibility) and `alias` is
/// the already-quoted output column name (via
/// [`super::resolution::quote_ident`]). [`SelectItem::render`] emits no leading
/// indentation, so callers keep control of clause layout.
pub(super) struct SelectItem {
    expr: String,
    cast: Option<String>,
    alias: String,
}

impl SelectItem {
    /// Build a select item. `cast` is the dimension/metric/fact `output_type`
    /// (rendered as `CAST(expr AS <cast>)` when `Some`).
    pub(super) fn new(expr: String, cast: Option<String>, alias: String) -> Self {
        Self { expr, cast, alias }
    }

    /// Write `CAST(expr AS <cast>)` (or bare `expr`) into `out`. The single
    /// source of the CAST-wrap rendering, shared by [`Self::rendered_expr`] and
    /// [`Self::render`] so they cannot diverge (the E-1 invariant lives here);
    /// writing into the caller's buffer keeps `render` to one allocation and
    /// avoids cloning `expr` in the common no-cast case.
    fn write_expr(&self, out: &mut String) {
        match &self.cast {
            Some(ty) => {
                out.push_str("CAST(");
                out.push_str(&self.expr);
                out.push_str(" AS ");
                out.push_str(ty);
                out.push(')');
            }
            None => out.push_str(&self.expr),
        }
    }

    /// The rendered expression with the optional CAST wrap applied, WITHOUT the
    /// trailing `AS alias`: `CAST(expr AS <cast>)` when a cast is set, else
    /// `expr`. Use where the same expression must be repeated elsewhere in the
    /// query — e.g. a window `PARTITION BY` / `ORDER BY` that must reference a
    /// CTE column by its expression rather than the shadowing select alias
    /// (E-1, code-review 2026-07-11).
    pub(super) fn rendered_expr(&self) -> String {
        let mut out = String::with_capacity(self.expr.len() + 16);
        self.write_expr(&mut out);
        out
    }

    /// Render as `<rendered_expr> AS alias` — i.e. `CAST(expr AS <cast>) AS
    /// alias` when a cast is set, else `expr AS alias`. No leading indent — the
    /// caller prepends clause indentation.
    pub(super) fn render(&self) -> String {
        let mut out = String::with_capacity(self.expr.len() + self.alias.len() + 24);
        self.write_expr(&mut out);
        out.push_str(" AS ");
        out.push_str(&self.alias);
        out
    }
}

/// Append the base-table FROM clause: `<lead>FROM <qualified-table> [AS
/// <alias>]`.
///
/// `lead` is the whitespace before `FROM` — `"\n"` at the top level, `"\n    "`
/// inside a CTE. The base table is qualified + quoted via
/// [`qualify_and_quote_table_ref`]; the first declared table's alias is
/// appended as `AS <quote_ident>` when present. Shared by the base, facts,
/// semi-additive, and window emitters (§6.2). The materialization renderer
/// intentionally does not use this — it selects from the pre-aggregated table
/// with no alias.
pub(super) fn push_from_base(sql: &mut String, def: &SemanticViewDefinition, lead: &str) {
    sql.push_str(lead);
    sql.push_str("FROM ");
    sql.push_str(&qualify_and_quote_table_ref(def.base_table(), def));
    if let Some(base_ref) = def.tables.first() {
        sql.push_str(" AS ");
        sql.push_str(&quote_ident(&base_ref.alias));
    }
}

/// Append an ordinal `GROUP BY` for `n` grouping columns: `<lead>GROUP BY\n`
/// then `<item_indent>1,\n<item_indent>2,…`. No-op when `n == 0`.
///
/// Ordinal grouping (never expressions) is what defends the base + CTE
/// aggregation paths from the E-1 alias-shadowing pitfall — `GROUP BY 1` can't
/// be captured by a same-named physical column the way `GROUP BY "region"`
/// could. Callers own the decision of WHETHER to group (the emptiness guard
/// differs by strategy: the base path needs both dimensions and metrics, the
/// CTE paths need only dimensions). `lead` is the whitespace before `GROUP BY`
/// (`"\n"` flat, `"\n    "` in a CTE); `item_indent` the per-ordinal indent
/// (`"    "` flat, `"        "` in a CTE).
pub(super) fn push_group_by_ordinals(sql: &mut String, n: usize, lead: &str, item_indent: &str) {
    if n == 0 {
        return;
    }
    sql.push_str(lead);
    sql.push_str("GROUP BY\n");
    let items: Vec<String> = (1..=n).map(|i| format!("{item_indent}{i}")).collect();
    sql.push_str(&items.join(",\n"));
}

#[cfg(test)]
mod tests {
    use super::{push_group_by_ordinals, SelectItem};

    #[test]
    fn renders_without_cast() {
        let item = SelectItem::new("o.region".to_string(), None, "\"region\"".to_string());
        assert_eq!(item.render(), "o.region AS \"region\"");
    }

    #[test]
    fn renders_with_cast() {
        let item = SelectItem::new(
            "SUM(o.amount)".to_string(),
            Some("DECIMAL(18,2)".to_string()),
            "\"revenue\"".to_string(),
        );
        assert_eq!(
            item.render(),
            "CAST(SUM(o.amount) AS DECIMAL(18,2)) AS \"revenue\""
        );
    }

    #[test]
    fn rendered_expr_omits_alias() {
        // No cast: the bare expression.
        let plain = SelectItem::new("upper(o.region)".to_string(), None, "\"r\"".to_string());
        assert_eq!(plain.rendered_expr(), "upper(o.region)");
        // With cast: the CAST wrap, still no `AS alias`. This is what the E-1
        // window PARTITION/ORDER clauses repeat instead of the select alias.
        let cast = SelectItem::new(
            "o.d".to_string(),
            Some("DATE".to_string()),
            "\"d\"".to_string(),
        );
        assert_eq!(cast.rendered_expr(), "CAST(o.d AS DATE)");
        assert_eq!(cast.render(), "CAST(o.d AS DATE) AS \"d\"");
    }

    /// Byte-identical to the pre-refactor idiom: the caller's
    /// `format!("    {}", item.render())` must equal the old
    /// `format!("    {} AS {}", final_expr, alias)` for both branches.
    #[test]
    fn matches_legacy_indented_format() {
        let alias = "\"m\"".to_string();
        // no cast
        let legacy_expr = "expr".to_string();
        let legacy = format!("    {} AS {}", legacy_expr, alias);
        let item = SelectItem::new("expr".to_string(), None, alias.clone());
        assert_eq!(format!("    {}", item.render()), legacy);
        // with cast (legacy pre-wraps the expr into final_expr)
        let legacy_final = format!("CAST({} AS {})", "expr", "INT");
        let legacy_cast = format!("    {} AS {}", legacy_final, alias);
        let item_cast = SelectItem::new("expr".to_string(), Some("INT".to_string()), alias.clone());
        assert_eq!(format!("    {}", item_cast.render()), legacy_cast);
    }

    #[test]
    fn group_by_ordinals_flat_and_cte_indents() {
        // Flat (top-level) layout: "\nGROUP BY\n" + 4-space ordinals.
        let mut flat = String::new();
        push_group_by_ordinals(&mut flat, 3, "\n", "    ");
        assert_eq!(flat, "\nGROUP BY\n    1,\n    2,\n    3");
        // CTE layout: "\n    GROUP BY\n" + 8-space ordinals.
        let mut cte = String::new();
        push_group_by_ordinals(&mut cte, 2, "\n    ", "        ");
        assert_eq!(cte, "\n    GROUP BY\n        1,\n        2");
    }

    #[test]
    fn group_by_ordinals_zero_is_noop() {
        let mut s = String::from("prefix");
        push_group_by_ordinals(&mut s, 0, "\n", "    ");
        assert_eq!(s, "prefix");
    }

    // push_from_base is covered end-to-end (byte-identical) by the exact-string
    // emission tests in sql_gen / semi_additive / window; a direct unit test
    // would need a full SemanticViewDefinition fixture for little added signal.
}
