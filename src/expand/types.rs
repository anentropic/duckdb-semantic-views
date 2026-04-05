use std::fmt;

/// A request to expand a semantic view into SQL.
///
/// Contains the names of dimensions and metrics to include in the query.
/// At least one dimension or one metric must be specified. Supported modes:
/// - Dimensions only: `SELECT DISTINCT` (no aggregation)
/// - Metrics only: global aggregate (no `GROUP BY`)
/// - Both: grouped aggregation with `GROUP BY`
#[derive(Debug, Clone)]
pub struct QueryRequest {
    pub dimensions: Vec<String>,
    pub metrics: Vec<String>,
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
}

impl fmt::Display for ExpandError {
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
        }
    }
}

impl std::error::Error for ExpandError {}
