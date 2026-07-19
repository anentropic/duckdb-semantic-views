use std::fmt;
use std::marker::PhantomData;

/// A query-request name (dimension or metric) with case- **and quote**-
/// insensitive equality and hashing.
///
/// Semantic-view names are matched under `DuckDB`'s identifier rule — case is
/// ignored, and a double-quoted name matches its unquoted spelling (`"Region"`,
/// `REGION`, `region` are the same name) — so this newtype provides
/// `PartialEq`/`Eq`/`Hash` on the canonical key from
/// [`crate::ident::normalize_ident_part`], the same rule
/// [`crate::ident::ident_matches`] and the resolution layer use. This
/// centralizes the ad-hoc `eq_ignore_ascii_case` / `to_ascii_lowercase` calls
/// that used to live throughout the resolution code (and closes the residual
/// gap where those folded case but did not strip quotes — TECH-DEBT #28
/// Slice 3). The `K` kind marker (see [`DimensionName`] and [`MetricName`])
/// keeps the flavors distinct at the type level so a dimension name can't be
/// passed where a metric name is expected — one impl, several types (R-7,
/// code-review 2026-07-11, replacing the former per-flavor copy-paste twins).
pub struct CiName<K> {
    raw: String,
    // `fn() -> K` keeps `CiName<K>: Send + Sync` regardless of `K` and marks the
    // kind purely at compile time (the marker types are never constructed).
    _kind: PhantomData<fn() -> K>,
}

impl<K> CiName<K> {
    pub fn new(s: impl Into<String>) -> Self {
        Self {
            raw: s.into(),
            _kind: PhantomData,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// The canonical match key: quote-stripped and ASCII-case-folded via
    /// [`crate::ident::normalize_ident_part`]. Used by `Hash` for the (rare)
    /// quoted path; `Eq` uses the equivalent [`crate::ident::ident_matches`],
    /// which is allocation-free when neither side is quoted.
    fn key(&self) -> String {
        crate::ident::normalize_ident_part(&self.raw)
    }
}

impl<K> Clone for CiName<K> {
    fn clone(&self) -> Self {
        Self::new(self.raw.clone())
    }
}

impl<K> fmt::Debug for CiName<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CiName").field(&self.raw).finish()
    }
}

impl<K> PartialEq for CiName<K> {
    fn eq(&self, other: &Self) -> bool {
        // Allocation-free when neither side is quoted (plain
        // `eq_ignore_ascii_case`); only a quoted side takes the
        // strip-and-normalize path — see `ident::ident_matches`.
        crate::ident::ident_matches(&self.raw, &other.raw)
    }
}

impl<K> Eq for CiName<K> {}

impl<K> std::hash::Hash for CiName<K> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Must agree with `PartialEq` — equal names (any case/quoting) hash
        // identically — while staying allocation-free on the common unquoted
        // path. Hash the canonical key's bytes one at a time: for an unquoted
        // name that is exactly its ASCII-lowercased bytes (no allocation, and
        // byte-identical to the pre-quote-aware impl); a quoted name is
        // normalized (quotes stripped) first, so `"Region"` hashes like
        // `region` — consistent with the quote-insensitive `Eq`.
        if self.raw.as_bytes().contains(&b'"') {
            for b in self.key().bytes() {
                b.hash(state);
            }
        } else {
            for b in self.raw.bytes() {
                b.to_ascii_lowercase().hash(state);
            }
        }
    }
}

impl<K> fmt::Display for CiName<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.raw)
    }
}

impl<K> std::ops::Deref for CiName<K> {
    type Target = str;
    fn deref(&self) -> &str {
        &self.raw
    }
}

impl<K> AsRef<str> for CiName<K> {
    fn as_ref(&self) -> &str {
        &self.raw
    }
}

impl<K> From<String> for CiName<K> {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl<K> From<&str> for CiName<K> {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

/// Kind marker for [`DimensionName`]; never constructed.
pub enum DimensionKind {}

/// Kind marker for [`MetricName`]; never constructed.
pub enum MetricKind {}

/// Kind marker for [`FactName`]; never constructed.
pub enum FactKind {}

/// A dimension name with case- and quote-insensitive equality and hashing (see [`CiName`]).
pub type DimensionName = CiName<DimensionKind>;

/// A metric name with case- and quote-insensitive equality and hashing (see [`CiName`]).
pub type MetricName = CiName<MetricKind>;

/// A fact name with case- and quote-insensitive equality and hashing (see [`CiName`]).
pub type FactName = CiName<FactKind>;

/// A request to expand a semantic view into SQL.
///
/// Contains the names of dimensions and metrics to include in the query.
/// At least one dimension, metric, or fact must be specified. Supported modes:
/// - Dimensions only: `SELECT DISTINCT` (no aggregation)
/// - Metrics only: global aggregate (no `GROUP BY`)
/// - Both: grouped aggregation with `GROUP BY`
/// - Facts mode: row-level query (facts cannot be combined with metrics)
#[derive(Debug, Clone)]
pub struct QueryRequest {
    pub dimensions: Vec<DimensionName>,
    pub metrics: Vec<MetricName>,
    pub facts: Vec<FactName>,
}

/// A resolved dimension paired with its role-playing scoped alias, if any.
///
/// R-8 (code-review 2026-07-11): replaces the former parallel slices
/// `resolved_dims: &[&Dimension]` and `dim_scoped_aliases: &[Option<String>]`,
/// which were threaded together through several expansion functions and indexed
/// by position (`dim_scoped_aliases[i]`) — a silent-wrong-results footgun if the
/// two ever fell out of sync. Zipping them into one value makes the pairing
/// structural, so an index can't reach the wrong alias.
pub(crate) struct ResolvedDim<'a> {
    /// The resolved dimension definition (borrowed from the view definition).
    pub dim: &'a crate::model::Dimension,
    /// The role-playing scoped alias for this dimension's source table
    /// (e.g. `Some("a__dep_airport")`), or `None` when the table is not
    /// role-played for this query.
    pub scoped_alias: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimension_name_case_insensitive_eq() {
        assert_eq!(DimensionName::new("Foo"), DimensionName::new("foo"));
        assert_eq!(DimensionName::new("FOO"), DimensionName::new("foo"));
        assert_ne!(DimensionName::new("foo"), DimensionName::new("bar"));
    }

    #[test]
    fn dimension_name_case_insensitive_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(DimensionName::new("Foo"));
        assert!(set.contains(&DimensionName::new("foo")));
        assert!(set.contains(&DimensionName::new("FOO")));
        assert!(!set.contains(&DimensionName::new("bar")));
    }

    #[test]
    fn dimension_name_quote_insensitive_eq_and_hash() {
        use std::collections::HashSet;
        // A double-quoted name matches its unquoted spelling (quotes stripped +
        // case folded), consistent with `ident::ident_matches` — TECH-DEBT #28.
        assert_eq!(
            DimensionName::new("\"Region\""),
            DimensionName::new("region")
        );
        assert_eq!(
            DimensionName::new("\"REGION\""),
            DimensionName::new("Region")
        );
        let mut set = HashSet::new();
        set.insert(DimensionName::new("region"));
        assert!(set.contains(&DimensionName::new("\"Region\"")));
        // A quoted name that carries a space still matches its unquoted key.
        assert_eq!(
            MetricName::new("\"Total Revenue\""),
            MetricName::new("total revenue")
        );
    }

    #[test]
    fn metric_name_case_insensitive_eq() {
        assert_eq!(MetricName::new("Revenue"), MetricName::new("revenue"));
        assert_ne!(MetricName::new("revenue"), MetricName::new("cost"));
    }

    #[test]
    fn metric_name_case_insensitive_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(MetricName::new("Revenue"));
        assert!(set.contains(&MetricName::new("revenue")));
        assert!(!set.contains(&MetricName::new("cost")));
    }

    #[test]
    fn dimension_name_display() {
        let name = DimensionName::new("Region");
        assert_eq!(format!("{name}"), "Region");
    }

    #[test]
    fn metric_name_deref_to_str() {
        let name = MetricName::new("total_revenue");
        let s: &str = &name;
        assert_eq!(s, "total_revenue");
    }

    #[test]
    fn dimension_name_from_string() {
        let name: DimensionName = "foo".into();
        assert_eq!(name.as_str(), "foo");
        let name2: DimensionName = String::from("bar").into();
        assert_eq!(name2.as_str(), "bar");
    }

    #[test]
    fn ci_name_shared_impl_covers_both_kinds() {
        // R-7 (code-review 2026-07-11): `DimensionName` and `MetricName` are now
        // `CiName<K>` aliases sharing one impl. Exercise the surface (Clone,
        // Deref, AsRef, case-insensitive Eq) through both kinds so the generic
        // impl stays covered for each.
        let dim = DimensionName::new("Region");
        let dim_clone = dim.clone();
        assert_eq!(dim, dim_clone);
        assert_eq!(dim, DimensionName::new("REGION")); // case-insensitive Eq
        assert_eq!(&*dim, "Region"); // Deref<Target = str>
        let as_ref: &str = dim.as_ref(); // AsRef<str>
        assert_eq!(as_ref, "Region");

        let met = MetricName::new("Total_Revenue");
        assert_eq!(met, MetricName::new("total_revenue"));
        assert_eq!(&*met.clone(), "Total_Revenue");
        assert_eq!(met.as_ref() as &str, "Total_Revenue");

        // Facts share the same case-insensitive impl (R-7 follow-up: the third
        // `CiName<K>` type the original change omitted, replacing the former
        // stringly `facts: Vec<String>`).
        let fact = FactName::new("Line_Total");
        assert_eq!(fact, FactName::new("line_total"));
        assert_eq!(&*fact.clone(), "Line_Total");
        assert_eq!(fact.as_ref() as &str, "Line_Total");
    }

    #[test]
    fn expand_error_stays_under_large_err_threshold() {
        // R-9 (code-review 2026-07-11): the two fattest variants (FanTrap,
        // MetricFanTrap) are boxed so `ExpandError` fits under clippy's
        // `result_large_err` threshold (128 bytes) and the `Result<_,
        // ExpandError>` allows could be dropped. Pin the size so a future fat
        // variant can't silently reintroduce the bloat (box it instead).
        assert!(
            std::mem::size_of::<ExpandError>() <= 128,
            "ExpandError is {} bytes (> 128); box the newly-added fat variant (see R-9)",
            std::mem::size_of::<ExpandError>()
        );
    }
}

/// Detail payload for [`ExpandError::FanTrap`], boxed so the enum stays small
/// (R-9, code-review 2026-07-11 — this variant was one of the two fattest).
#[derive(Debug)]
pub struct FanTrapError {
    pub view_name: String,
    pub metric_name: String,
    pub metric_table: String,
    pub dimension_name: String,
    pub dimension_table: String,
    pub relationship_name: String,
}

/// Detail payload for [`ExpandError::MetricFanTrap`], boxed so the enum stays
/// small (R-9, code-review 2026-07-11 — the other fat variant).
#[derive(Debug)]
pub struct MetricFanTrapError {
    pub view_name: String,
    pub metric_name: String,
    pub metric_table: String,
    pub other_metric_name: String,
    pub other_metric_table: String,
    pub relationship_name: String,
}

/// Errors that can occur during semantic view expansion.
#[derive(Debug)]
pub enum ExpandError {
    /// The request contained neither dimensions nor metrics.
    EmptyRequest { view_name: String },
    /// A requested dimension name does not exist in the view definition.
    UnknownDimension {
        view_name: String,
        name: String,
        available: Vec<String>,
        suggestion: Option<String>,
    },
    /// A requested metric name does not exist in the view definition.
    UnknownMetric {
        view_name: String,
        name: String,
        available: Vec<String>,
        suggestion: Option<String>,
    },
    /// A dimension name was requested more than once.
    DuplicateDimension { view_name: String, name: String },
    /// A metric name was requested more than once.
    DuplicateMetric { view_name: String, name: String },
    /// A metric aggregates across a one-to-many boundary, risking inflated results.
    FanTrap { detail: Box<FanTrapError> },
    /// Two queried metrics sit at different grains (source tables) and the
    /// join path between those tables crosses a fan-out edge: joining both
    /// source tables multiplies `metric_table`'s rows, silently inflating
    /// `metric_name` (fan trap / chasm trap between metric grains).
    MetricFanTrap { detail: Box<MetricFanTrapError> },
    /// A metric aggregates a table that fans out relative to the query's base
    /// (root) table. The generated SQL is always anchored `FROM <root>`, so if
    /// the metric's source table is a parent/ancestor of the root across a
    /// many-to-one edge, the metric's rows are duplicated once per root row and
    /// the aggregate is silently inflated — even when the metric is queried
    /// alone with no other metric or dimension to trigger the pairwise checks
    /// (EXP-1, code-review 2026-07-18).
    RootGrainFanTrap {
        view_name: String,
        metric_name: String,
        metric_table: String,
        relationship_name: String,
    },
    /// The stored definition's relationship graph could not be rebuilt at
    /// query time, so safety checks (fan-trap detection) cannot run.
    UncheckableDefinition { view_name: String, reason: String },
    /// A dimension from a role-playing table is ambiguous because multiple
    /// relationships reach that table and no co-queried metric provides USING
    /// context to disambiguate.
    AmbiguousPath {
        view_name: String,
        dimension_name: String,
        dimension_table: String,
        available_relationships: Vec<String>,
    },
    /// A dimension whose table is reached *only through* a role-playing table
    /// (a descendant of it) — the role cannot be inferred and, unlike a
    /// dimension directly on the role-playing table, cannot be scoped by a
    /// co-queried metric's USING (EXP-4, code-review 2026-07-18). Reached one
    /// hop past the `AmbiguousPath` case, this previously bound silently to the
    /// first-declared relationship.
    AmbiguousDescendantPath {
        view_name: String,
        dimension_name: String,
        dimension_table: String,
        role_playing_table: String,
        available_relationships: Vec<String>,
    },
    /// A fact whose source table is (or is reached only through) a role-playing
    /// table. Facts carry no USING context, so the role is unresolvable
    /// (EXP-5, code-review 2026-07-18); previously bound silently to the
    /// first-declared relationship.
    AmbiguousFactPath {
        view_name: String,
        fact_name: String,
        fact_table: String,
        role_playing_table: String,
        available_relationships: Vec<String>,
    },
    /// A requested metric is marked PRIVATE and cannot be queried directly.
    PrivateMetric { view_name: String, name: String },
    /// A requested fact is marked PRIVATE and cannot be queried directly.
    PrivateFact { view_name: String, name: String },
    /// Facts and metrics cannot be combined in the same query.
    FactsMetricsMutualExclusion { view_name: String },
    /// A requested fact name does not exist in the view definition.
    UnknownFact {
        view_name: String,
        name: String,
        available: Vec<String>,
        suggestion: Option<String>,
    },
    /// A fact name was requested more than once.
    DuplicateFact { view_name: String, name: String },
    /// A fact query references objects from incompatible table paths.
    FactPathViolation {
        view_name: String,
        table_a: String,
        table_b: String,
    },
    /// Window function metrics cannot be mixed with aggregate metrics.
    WindowAggregateMixing {
        view_name: String,
        window_metrics: Vec<String>,
        aggregate_metrics: Vec<String>,
    },
    /// A dimension required by a window metric (EXCLUDING or ORDER BY) is not in the query.
    WindowMetricRequiredDimension {
        view_name: String,
        metric_name: String,
        dimension_name: String,
        reason: String,
    },
    /// The catalog `RwLock` is poisoned (a previous thread panicked while holding the lock).
    CatalogPoisoned { view_name: String },
    /// A cycle was detected in derived metric or fact dependencies at query expansion time.
    CycleDetected {
        view_name: String,
        cycle_description: String,
    },
    /// Derived metric nesting exceeds the maximum allowed depth.
    MaxDepthExceeded {
        view_name: String,
        depth: usize,
        max_depth: usize,
    },
    /// A metric co-queried with an active semi-additive metric cannot be
    /// decomposed for the snapshot CTE (SG-5). The CTE captures each metric's
    /// inner expression per row and re-aggregates it outside the snapshot
    /// filter, which is only sound for a single bare aggregate call
    /// `FUNC(args)` with FUNC in SUM/COUNT/AVG/MIN/MAX, no `*`, no DISTINCT.
    SemiAdditiveCoQueryUnsupported {
        view_name: String,
        metric_name: String,
        metric_expr: String,
        semi_metric_name: String,
        reason: String,
    },
    /// An active semi-additive metric's own expression cannot be decomposed
    /// for the snapshot CTE (same shape requirements as co-queried metrics).
    SemiAdditiveUnsupportedExpression {
        view_name: String,
        metric_name: String,
        metric_expr: String,
        reason: String,
    },
    /// A `COUNT(*)` metric on a non-base source table cannot be made safe
    /// (SG-8). Synthesized joins are LEFT JOINs, so the source table is
    /// NULL-extended by one row per unmatched base row and `COUNT(*)` would
    /// silently over-count. The expansion rewrites such metrics to
    /// `COUNT(<first PK column>)`, which requires the source table to declare
    /// a PRIMARY KEY.
    CountStarRequiresPrimaryKey {
        view_name: String,
        metric_name: String,
        table_alias: String,
    },
}

impl fmt::Display for ExpandError {
    #[allow(clippy::too_many_lines)]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // R-16 (code-review 2026-07-11): this arm is the single source of the
            // empty-request wording. `QueryError::EmptyRequest` renders by
            // delegating to it, so the two can no longer drift apart (they had:
            // this side lacked the `facts` option and the DESCRIBE hint). Both
            // are reachable — the FFI binder short-circuits with the QueryError
            // form; a direct `expand()` call hits this form.
            Self::EmptyRequest { view_name } => {
                write!(
                    f,
                    "semantic view '{view_name}': specify at least dimensions := [...], metrics := [...], or facts := [...]."
                )?;
                write!(
                    f,
                    " Run DESCRIBE SEMANTIC VIEW {view_name} to see available dimensions, metrics, and facts."
                )
            }
            Self::UnknownDimension {
                view_name,
                name,
                available,
                suggestion,
            } => {
                write!(
                    f,
                    "semantic view '{view_name}': unknown dimension '{name}'. Available: [{}]",
                    available.join(", ")
                )?;
                if let Some(s) = suggestion {
                    write!(f, ". Did you mean '{s}'?")?;
                }
                Ok(())
            }
            Self::UnknownMetric {
                view_name,
                name,
                available,
                suggestion,
            } => {
                write!(
                    f,
                    "semantic view '{view_name}': unknown metric '{name}'. Available: [{}]",
                    available.join(", ")
                )?;
                if let Some(s) = suggestion {
                    write!(f, ". Did you mean '{s}'?")?;
                }
                Ok(())
            }
            Self::DuplicateDimension { view_name, name } => {
                write!(
                    f,
                    "semantic view '{view_name}': duplicate dimension '{name}'"
                )
            }
            Self::DuplicateMetric { view_name, name } => {
                write!(f, "semantic view '{view_name}': duplicate metric '{name}'")
            }
            Self::FanTrap { detail } => {
                let FanTrapError {
                    view_name,
                    metric_name,
                    metric_table,
                    dimension_name,
                    dimension_table,
                    relationship_name,
                } = &**detail;
                write!(
                    f,
                    "semantic view '{view_name}': fan trap detected -- metric '{metric_name}' \
                     (table '{metric_table}') would be duplicated when joined to dimension \
                     '{dimension_name}' (table '{dimension_table}') via relationship \
                     '{relationship_name}' (many-to-one cardinality, inferred: FK is not PK/UNIQUE). \
                     This would inflate aggregation results. \
                     Remove the dimension, use a metric from the same table, or restructure the \
                     relationship."
                )
            }
            Self::MetricFanTrap { detail } => {
                let MetricFanTrapError {
                    view_name,
                    metric_name,
                    metric_table,
                    other_metric_name,
                    other_metric_table,
                    relationship_name,
                } = &**detail;
                write!(
                    f,
                    "semantic view '{view_name}': fan trap detected -- metric '{metric_name}' \
                     (table '{metric_table}') and metric '{other_metric_name}' (table \
                     '{other_metric_table}') aggregate at different grains: joining their source \
                     tables via relationship '{relationship_name}' (many-to-one cardinality) \
                     duplicates rows of '{metric_table}' and would inflate '{metric_name}'. \
                     Query the metrics separately, or restructure the relationship."
                )
            }
            Self::RootGrainFanTrap {
                view_name,
                metric_name,
                metric_table,
                relationship_name,
            } => {
                write!(
                    f,
                    "semantic view '{view_name}': fan trap detected -- metric '{metric_name}' \
                     (table '{metric_table}') aggregates a table that fans out relative to the \
                     query's base table via relationship '{relationship_name}' (many-to-one \
                     cardinality): the query is anchored FROM the base table, so '{metric_table}' \
                     rows are duplicated once per base-table row and '{metric_name}' would be \
                     inflated. Query this metric at the base table's grain, or restructure the \
                     relationship."
                )
            }
            Self::UncheckableDefinition { view_name, reason } => {
                write!(
                    f,
                    "semantic view '{view_name}': cannot verify the query is safe from fan traps \
                     -- the stored definition's relationship graph could not be built: {reason}. \
                     The definition likely predates current validation rules; re-create it with \
                     CREATE OR REPLACE SEMANTIC VIEW."
                )
            }
            Self::AmbiguousPath {
                view_name,
                dimension_name,
                dimension_table,
                available_relationships,
            } => {
                write!(
                    f,
                    "semantic view '{view_name}': dimension '{dimension_name}' is ambiguous -- \
                     table '{dimension_table}' is reached via multiple relationships: [{}]. \
                     Specify a metric with USING to disambiguate, or use a dimension from a \
                     non-ambiguous table.",
                    available_relationships.join(", ")
                )
            }
            Self::AmbiguousDescendantPath {
                view_name,
                dimension_name,
                dimension_table,
                role_playing_table,
                available_relationships,
            } => {
                write!(
                    f,
                    "semantic view '{view_name}': dimension '{dimension_name}' is ambiguous -- \
                     its table '{dimension_table}' is reachable only through the role-playing \
                     table '{role_playing_table}', which is joined via multiple relationships: \
                     [{}]. The role cannot be inferred for a descendant table; query a dimension \
                     directly on '{role_playing_table}' with a metric USING one of those \
                     relationships, or give the target table a distinct alias per role.",
                    available_relationships.join(", ")
                )
            }
            Self::AmbiguousFactPath {
                view_name,
                fact_name,
                fact_table,
                role_playing_table,
                available_relationships,
            } => {
                write!(
                    f,
                    "semantic view '{view_name}': fact '{fact_name}' is ambiguous -- reaching its \
                     table '{fact_table}' requires the role-playing table '{role_playing_table}', \
                     joined via multiple relationships: [{}], and fact queries carry no USING \
                     context to pick a role. Restructure the relationship or query via a \
                     non-role-playing table.",
                    available_relationships.join(", ")
                )
            }
            Self::PrivateMetric { view_name, name } => {
                write!(
                    f,
                    "semantic view '{view_name}': metric '{name}' is private and cannot be queried directly. \
                     Private metrics can only be used in derived metric expressions."
                )
            }
            Self::PrivateFact { view_name, name } => {
                write!(
                    f,
                    "semantic view '{view_name}': fact '{name}' is private and cannot be queried directly. \
                     Private facts can only be used in derived expressions."
                )
            }
            Self::FactsMetricsMutualExclusion { view_name } => {
                write!(
                    f,
                    "semantic view '{view_name}': cannot combine facts and metrics in the same query. \
                     Use facts := [...] OR metrics := [...], not both."
                )
            }
            Self::UnknownFact {
                view_name,
                name,
                available,
                suggestion,
            } => {
                write!(
                    f,
                    "semantic view '{view_name}': unknown fact '{name}'. Available: [{}]",
                    available.join(", ")
                )?;
                if let Some(s) = suggestion {
                    write!(f, ". Did you mean '{s}'?")?;
                }
                Ok(())
            }
            Self::DuplicateFact { view_name, name } => {
                write!(f, "semantic view '{view_name}': duplicate fact '{name}'")
            }
            Self::FactPathViolation {
                view_name,
                table_a,
                table_b,
            } => {
                write!(
                    f,
                    "semantic view '{view_name}': fact query references objects from incompatible \
                     table paths -- tables '{table_a}' and '{table_b}' are not on the same \
                     root-to-leaf path in the relationship tree"
                )
            }
            Self::WindowAggregateMixing {
                view_name,
                window_metrics,
                aggregate_metrics,
            } => {
                write!(
                    f,
                    "semantic view '{view_name}': cannot mix window function metrics [{}] \
                     with aggregate metrics [{}] in the same query",
                    window_metrics.join(", "),
                    aggregate_metrics.join(", ")
                )
            }
            Self::WindowMetricRequiredDimension {
                view_name,
                metric_name,
                dimension_name,
                reason,
            } => {
                write!(
                    f,
                    "semantic view '{view_name}': window function metric '{metric_name}' requires \
                     dimension '{dimension_name}' to be included in the query (used in {reason})"
                )
            }
            Self::CatalogPoisoned { view_name } => {
                write!(
                    f,
                    "semantic view '{view_name}': internal error -- catalog lock is poisoned \
                     (a previous operation panicked). Restart DuckDB to recover."
                )
            }
            Self::CycleDetected {
                view_name,
                cycle_description,
            } => {
                write!(
                    f,
                    "semantic view '{view_name}': cycle detected in metric/fact dependencies \
                     during query expansion: {cycle_description}"
                )
            }
            Self::MaxDepthExceeded {
                view_name,
                depth,
                max_depth,
            } => {
                write!(
                    f,
                    "semantic view '{view_name}': derived metric nesting depth {depth} exceeds \
                     maximum allowed depth of {max_depth}"
                )
            }
            Self::SemiAdditiveCoQueryUnsupported {
                view_name,
                metric_name,
                metric_expr,
                semi_metric_name,
                reason,
            } => {
                write!(
                    f,
                    "semantic view '{view_name}': metric '{metric_name}' (expression: \
                     {metric_expr}) cannot be co-queried with semi-additive metric \
                     '{semi_metric_name}': {reason}. Snapshot expansion for NON ADDITIVE BY \
                     requires every co-queried metric to be a single aggregate call \
                     SUM/COUNT/AVG/MIN/MAX(<expression>) without '*', DISTINCT, or surrounding \
                     expression text. Query '{metric_name}' and '{semi_metric_name}' separately."
                )
            }
            Self::SemiAdditiveUnsupportedExpression {
                view_name,
                metric_name,
                metric_expr,
                reason,
            } => {
                write!(
                    f,
                    "semantic view '{view_name}': semi-additive metric '{metric_name}' \
                     (expression: {metric_expr}) cannot be expanded: {reason}. NON ADDITIVE BY \
                     snapshot expansion requires the metric to be a single aggregate call \
                     SUM/COUNT/AVG/MIN/MAX(<expression>) without '*' or DISTINCT."
                )
            }
            Self::CountStarRequiresPrimaryKey {
                view_name,
                metric_name,
                table_alias,
            } => {
                write!(
                    f,
                    "semantic view '{view_name}': metric '{metric_name}' uses COUNT(*) on joined \
                     table '{table_alias}'. The generated LEFT JOIN produces one NULL-extended \
                     row per base-table row with no match in '{table_alias}', which COUNT(*) \
                     would count -- so the expansion rewrites COUNT(*) to COUNT(<primary key>) \
                     for non-base tables, but table '{table_alias}' has no PRIMARY KEY declared \
                     in the TABLES clause. Add PRIMARY KEY (cols) to '{table_alias}' or use an \
                     explicit column: COUNT({table_alias}.<column>)."
                )
            }
        }
    }
}

impl std::error::Error for ExpandError {}
