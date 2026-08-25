use models::{QueryOptimizerResult, QueryTabState};

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

/// Store a parsed optimizer result on the tab with the given id, if present.
/// Clears any previously stored raw fallback so the two never coexist.
pub fn store_optimizer_result_on_tab(
    tabs: &mut [QueryTabState],
    tab_id: u64,
    result: QueryOptimizerResult,
) {
    if let Some(tab) = tabs.iter_mut().find(|tab| tab.id == tab_id) {
        tab.optimizer_result = Some(result);
        tab.optimizer_raw_response = None;
    }
}

/// Store the raw optimizer response on the tab with the given id, if present.
/// Used to render a fallback card when the AI returned unstructured (non-JSON)
/// output that could not be parsed into a `QueryOptimizerResult`.
pub fn store_optimizer_raw_response_on_tab(tabs: &mut [QueryTabState], tab_id: u64, raw: String) {
    if let Some(tab) = tabs.iter_mut().find(|tab| tab.id == tab_id) {
        tab.optimizer_raw_response = Some(raw);
        tab.optimizer_result = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use models::{OptimizerSeverity, QueryTabState, RecommendationCategory};

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

    #[test]
    fn stores_result_on_active_tab() {
        let mut tabs = vec![QueryTabState::default()];
        let result = QueryOptimizerResult {
            summary: "s".to_string(),
            recommendations: vec![],
            rewritten_sql: None,
        };
        store_optimizer_result_on_tab(&mut tabs, 0, result.clone());
        assert_eq!(tabs[0].optimizer_result, Some(result));
    }

    #[test]
    fn stores_raw_response_on_parse_failure() {
        let mut tabs = vec![QueryTabState::default()];
        let raw = "this is not json at all".to_string();
        store_optimizer_raw_response_on_tab(&mut tabs, 0, raw.clone());
        assert_eq!(tabs[0].optimizer_raw_response, Some(raw));
        assert!(tabs[0].optimizer_result.is_none());
    }

    #[test]
    fn storing_result_clears_raw_fallback() {
        let mut tabs = vec![QueryTabState::default()];
        store_optimizer_raw_response_on_tab(&mut tabs, 0, "raw".to_string());
        let result = QueryOptimizerResult {
            summary: "s".to_string(),
            recommendations: vec![],
            rewritten_sql: None,
        };
        store_optimizer_result_on_tab(&mut tabs, 0, result.clone());
        assert_eq!(tabs[0].optimizer_result, Some(result));
        assert!(tabs[0].optimizer_raw_response.is_none());
    }
}
