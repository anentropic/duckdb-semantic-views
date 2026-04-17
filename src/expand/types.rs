use std::fmt;

/// A dimension name with case-insensitive equality and hashing.
///
/// Wraps a `String` and provides `PartialEq`/`Eq`/`Hash` based on ASCII-lowercased form.
/// This centralizes the ad-hoc `eq_ignore_ascii_case` / `to_ascii_lowercase` calls
/// throughout the resolution code.
#[derive(Debug, Clone)]
pub struct DimensionName(String);

impl DimensionName {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq for DimensionName {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl Eq for DimensionName {}

impl std::hash::Hash for DimensionName {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for byte in self.0.bytes() {
            byte.to_ascii_lowercase().hash(state);
        }
    }
}

impl fmt::Display for DimensionName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::ops::Deref for DimensionName {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for DimensionName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for DimensionName {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for DimensionName {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// A metric name with case-insensitive equality and hashing.
///
/// Wraps a `String` and provides `PartialEq`/`Eq`/`Hash` based on ASCII-lowercased form.
/// This centralizes the ad-hoc `eq_ignore_ascii_case` / `to_ascii_lowercase` calls
/// throughout the resolution code.
#[derive(Debug, Clone)]
pub struct MetricName(String);

impl MetricName {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq for MetricName {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl Eq for MetricName {}

impl std::hash::Hash for MetricName {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for byte in self.0.bytes() {
            byte.to_ascii_lowercase().hash(state);
        }
    }
}

impl fmt::Display for MetricName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::ops::Deref for MetricName {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for MetricName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for MetricName {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for MetricName {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

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
    pub facts: Vec<String>,
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
    FanTrap {
        view_name: String,
        metric_name: String,
        metric_table: String,
        dimension_name: String,
        dimension_table: String,
        relationship_name: String,
    },
    /// A dimension from a role-playing table is ambiguous because multiple
    /// relationships reach that table and no co-queried metric provides USING
    /// context to disambiguate.
    AmbiguousPath {
        view_name: String,
        dimension_name: String,
        dimension_table: String,
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
}

impl fmt::Display for ExpandError {
    #[allow(clippy::too_many_lines)]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRequest { view_name } => {
                write!(
                    f,
                    "semantic view '{view_name}': specify at least dimensions := [...] or metrics := [...]"
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
            Self::FanTrap {
                view_name,
                metric_name,
                metric_table,
                dimension_name,
                dimension_table,
                relationship_name,
            } => {
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
        }
    }
}

impl std::error::Error for ExpandError {}
