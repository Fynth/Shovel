use models::QueryOptimizerResult;

/// Extract the first fenced ```json ... ``` block from a raw agent response.
/// Falls back to the whole string when no fence is present.
#[allow(dead_code)] // consumed by later optimizer tasks
pub fn extract_json_block(raw: &str) -> Option<String> {
    let start_marker = "```json";
    if let Some(start) = raw.find(start_marker) {
        let after = &raw[start + start_marker.len()..];
        if let Some(end) = after.find("```") {
            let block = after[..end].trim();
            if !block.is_empty() {
                return Some(block.to_string());
            }
        }
    }
    // No fence: try the whole string as JSON.
    if raw.trim().starts_with('{') {
        Some(raw.trim().to_string())
    } else {
        None
    }
}

/// Parse a raw agent response into a `QueryOptimizerResult`.
#[allow(dead_code)] // consumed by later optimizer tasks
pub fn parse_optimizer_result(raw: &str) -> Result<QueryOptimizerResult, String> {
    let block = extract_json_block(raw)
        .ok_or_else(|| "No JSON block found in the agent response.".to_string())?;
    serde_json::from_str(&block).map_err(|err| format!("Invalid optimizer JSON: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use models::{OptimizerSeverity, RecommendationCategory};

    #[test]
    fn parses_fenced_json_block() {
        let raw = "Here is the analysis:\n```json\n{\"summary\":\"s\",\"recommendations\":[],\"rewritten_sql\":null}\n```\n";
        let result = parse_optimizer_result(raw).unwrap();
        assert_eq!(result.summary, "s");
        assert!(result.recommendations.is_empty());
        assert!(result.rewritten_sql.is_none());
    }

    #[test]
    fn parses_plain_json_without_fence() {
        let raw = "{\"summary\":\"s\",\"recommendations\":[],\"rewritten_sql\":\"SELECT 1\"}";
        let result = parse_optimizer_result(raw).unwrap();
        assert_eq!(result.rewritten_sql.as_deref(), Some("SELECT 1"));
    }

    #[test]
    fn rejects_invalid_json() {
        let raw = "this is not json at all";
        assert!(parse_optimizer_result(raw).is_err());
    }

    #[test]
    fn parses_recommendation_fields() {
        let raw = r#"{"summary":"s","recommendations":[{"severity":"critical","category":"join","title":"t","detail":"d","suggested_index":"CREATE INDEX i ON t(c)"}],"rewritten_sql":null}"#;
        let result = parse_optimizer_result(raw).unwrap();
        assert_eq!(result.recommendations.len(), 1);
        let rec = &result.recommendations[0];
        assert_eq!(rec.severity, OptimizerSeverity::Critical);
        assert_eq!(rec.category, RecommendationCategory::Join);
        assert_eq!(
            rec.suggested_index.as_deref(),
            Some("CREATE INDEX i ON t(c)")
        );
    }
}
