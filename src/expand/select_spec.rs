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

    /// Render as `CAST(expr AS <cast>) AS alias` (when a cast is set) or
    /// `expr AS alias`. No leading indent — the caller prepends clause
    /// indentation.
    pub(super) fn render(&self) -> String {
        match &self.cast {
            Some(ty) => format!("CAST({} AS {}) AS {}", self.expr, ty, self.alias),
            None => format!("{} AS {}", self.expr, self.alias),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SelectItem;

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
}
