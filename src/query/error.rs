use std::fmt;

use crate::expand::ExpandError;

/// Errors that can occur when executing a semantic view query.
#[derive(Debug)]
pub enum QueryError {
    /// The requested semantic view does not exist in the catalog.
    ViewNotFound {
        name: String,
        suggestion: Option<String>,
        available: Vec<String>,
    },
    /// The query specified neither dimensions nor metrics.
    EmptyRequest { view_name: String },
    /// The expansion engine returned an error.
    ExpandFailed { source: ExpandError },
    /// The expanded SQL failed to execute against DuckDB.
    SqlExecution {
        expanded_sql: String,
        duckdb_error: String,
    },
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ViewNotFound {
                name,
                suggestion,
                available,
            } => {
                write!(f, "Semantic view '{name}' not found.")?;
                if let Some(s) = suggestion {
                    write!(f, " Did you mean '{s}'?")?;
                }
                if !available.is_empty() {
                    write!(f, " Available views: [{}].", available.join(", "))?;
                }
                write!(
                    f,
                    " Run FROM list_semantic_views() to see all registered views."
                )
            }
            Self::EmptyRequest { view_name } => {
                write!(
                    f,
                    "semantic view '{view_name}': specify at least dimensions := [...] or metrics := [...]."
                )?;
                write!(
                    f,
                    " Run FROM describe_semantic_view('{view_name}') to see available dimensions and metrics."
                )
            }
            Self::ExpandFailed { source } => {
                write!(f, "{source}")
            }
            Self::SqlExecution {
                expanded_sql,
                duckdb_error,
            } => {
                write!(
                    f,
                    "SQL execution failed: {duckdb_error}\nExpanded SQL:\n{expanded_sql}"
                )
            }
        }
    }
}

impl std::error::Error for QueryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ExpandFailed { source } => Some(source),
            _ => None,
        }
    }
}

impl From<ExpandError> for QueryError {
    fn from(source: ExpandError) -> Self {
        Self::ExpandFailed { source }
    }
}
