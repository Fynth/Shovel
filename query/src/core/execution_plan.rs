use database::SessionHandle;
use models::{DatabaseError, ExecutionPlan};

/// Execute an EXPLAIN query and return a parsed execution plan.
pub async fn execute_explain(
    handle: &SessionHandle,
    sql: &str,
    analyze: bool,
) -> Result<ExecutionPlan, DatabaseError> {
    let Some(explain) = handle.explain() else {
        return Err(DatabaseError::Unsupported(
            "explain is not supported for this session".into(),
        ));
    };
    explain.execute_explain(sql, analyze).await
}
