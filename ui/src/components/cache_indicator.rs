//! Semantic Cache UI Indicators
//!
//! Provides visual indicators for semantic cache hits, statistics, and settings.

use dioxus::prelude::*;

/// Default similarity threshold for cache lookups
#[allow(dead_code)]
pub const DEFAULT_SIMILARITY_THRESHOLD: f32 = 0.85;

/// Minimum similarity threshold
#[allow(dead_code)]
pub const MIN_SIMILARITY_THRESHOLD: f32 = 0.80;

/// Maximum similarity threshold
#[allow(dead_code)]
pub const MAX_SIMILARITY_THRESHOLD: f32 = 0.95;

/// Cache hit result information
#[derive(Clone, Debug, PartialEq)]
pub struct CacheHitInfo {
    /// Whether the response came from cache
    pub is_hit: bool,
    /// Similarity score (0.0-1.0) if available
    pub similarity_score: Option<f32>,
    /// Original cached query text
    pub cached_query: Option<String>,
}

/// Cache statistics for display
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CacheDisplayStats {
    /// Total number of cache entries
    pub total_entries: usize,
    /// Total number of cache hits
    pub total_hits: usize,
    /// Hit rate (0.0-1.0)
    pub hit_rate: f32,
    /// Whether the cache is warming up
    pub is_warming: bool,
}

/// Props for CacheIndicator component
#[derive(Props, Clone, PartialEq)]
pub struct CacheIndicatorProps {
    /// Cache hit information
    pub hit_info: CacheHitInfo,
    /// Whether to show the similarity score
    #[props(default = true)]
    pub show_score: bool,
    /// Whether to show the "View similar queries" button
    #[props(default = false)]
    pub show_similar_button: bool,
}

/// Cache indicator badge showing hit/miss status and similarity score.
///
/// Displays a small badge next to AI responses indicating whether the
/// response came from the semantic cache and the similarity score.
#[component]
pub fn CacheIndicator(props: CacheIndicatorProps) -> Element {
    let CacheIndicatorProps {
        hit_info,
        show_score,
        show_similar_button,
    } = props;

    if !hit_info.is_hit {
        return rsx! {};
    }

    let score_text = hit_info
        .similarity_score
        .map(|score| format!("{:.2} match", score));

    rsx! {
        div {
            class: "cache-indicator",
            span {
                class: "cache-indicator__badge",
                "Cached"
            }
            if show_score {
                if let Some(score) = score_text {
                    span {
                        class: "cache-indicator__score",
                        {score.to_string()}
                    }
                }
            }
            if show_similar_button {
                if let Some(_query) = hit_info.cached_query.as_ref() {
                    button {
                        class: "cache-indicator__similar-btn",
                        title: "View similar cached query",
                        onclick: move |e| {
                            e.stop_propagation();
                        },
                        "?"
                    }
                }
            }
        }
    }
}

/// Props for CacheWarmingIndicator component
#[derive(Props, Clone, PartialEq)]
pub struct CacheWarmingIndicatorProps {
    /// Whether the cache is currently warming
    pub is_warming: bool,
}

/// Shows a "Cache warming" indicator during first load of embedding model.
#[component]
pub fn CacheWarmingIndicator(props: CacheWarmingIndicatorProps) -> Element {
    if !props.is_warming {
        return rsx! {};
    }

    rsx! {
        div {
            class: "cache-warming",
            span {
                class: "cache-warming__dot cache-warming__dot--1",
            }
            span {
                class: "cache-warming__dot cache-warming__dot--2",
            }
            span {
                class: "cache-warming__dot cache-warming__dot--3",
            }
            span {
                class: "cache-warming__text",
                "Cache warming"
            }
        }
    }
}

/// Props for CacheStatsView component
#[derive(Props, Clone, PartialEq)]
pub struct CacheStatsViewProps {
    /// Cache statistics to display
    pub stats: CacheDisplayStats,
    /// Whether to show detailed stats
    #[props(default = false)]
    pub detailed: bool,
}

/// Cache statistics view showing total hits, hit rate, and cache size.
#[component]
pub fn CacheStatsView(props: CacheStatsViewProps) -> Element {
    let CacheStatsViewProps { stats, detailed } = props;

    let hit_rate_percent = (stats.hit_rate * 100.0).round() as u32;

    rsx! {
        div {
            class: "cache-stats",
            div {
                class: "cache-stats__row",
                span {
                    class: "cache-stats__label",
                    "Cache entries"
                }
                span {
                    class: "cache-stats__value",
                    "{stats.total_entries}"
                }
            }
            div {
                class: "cache-stats__row",
                span {
                    class: "cache-stats__label",
                    "Total hits"
                }
                span {
                    class: "cache-stats__value",
                    "{stats.total_hits}"
                }
            }
            div {
                class: "cache-stats__row",
                span {
                    class: "cache-stats__label",
                    "Hit rate"
                }
                span {
                    class: "cache-stats__value",
                    "{hit_rate_percent}%"
                }
            }
            if detailed {
                if stats.is_warming {
                    div {
                        class: "cache-stats__warming",
                        "Embedding model loading..."
                    }
                }
            }
        }
    }
}

/// Props for CacheSettings component
#[derive(Props, Clone, PartialEq)]
pub struct CacheSettingsProps {
    /// Whether the cache is enabled
    pub enabled: Signal<bool>,
    /// Similarity threshold (0.80-0.95)
    pub threshold: Signal<f32>,
    /// Callback when enabled changes
    pub on_enabled_change: EventHandler<bool>,
    /// Callback when threshold changes
    pub on_threshold_change: EventHandler<f32>,
}

/// Settings panel for semantic cache configuration.
///
/// Provides enable/disable toggle and threshold adjustment slider.
#[component]
pub fn CacheSettings(props: CacheSettingsProps) -> Element {
    let CacheSettingsProps {
        mut enabled,
        mut threshold,
        on_enabled_change,
        on_threshold_change,
    } = props;

    let threshold_percent = (threshold() * 100.0).round() as u32;

    rsx! {
        div {
            class: "cache-settings",
            div {
                class: "cache-settings__row",
                label {
                    class: "field__label",
                    r#for: "cache-enabled",
                    "Enable semantic cache"
                }
                input {
                    r#type: "checkbox",
                    id: "cache-enabled",
                    checked: enabled(),
                    onchange: move |e| {
                        let new_value = e.checked();
                        enabled.set(new_value);
                        on_enabled_change.call(new_value);
                    },
                }
            }
            div {
                class: "cache-settings__row",
                label {
                    class: "field__label",
                    r#for: "cache-threshold",
                    "Similarity threshold"
                }
                div {
                    class: "cache-settings__slider-container",
                    input {
                        r#type: "range",
                        id: "cache-threshold",
                        min: "{MIN_SIMILARITY_THRESHOLD}",
                        max: "{MAX_SIMILARITY_THRESHOLD}",
                        step: "0.01",
                        value: "{threshold()}",
                        onchange: move |e| {
                            let new_value = e.value().parse::<f32>().unwrap_or(DEFAULT_SIMILARITY_THRESHOLD);
                            let clamped = new_value.clamp(MIN_SIMILARITY_THRESHOLD, MAX_SIMILARITY_THRESHOLD);
                            threshold.set(clamped);
                            on_threshold_change.call(clamped);
                        },
                    }
                    span {
                        class: "cache-settings__threshold-value",
                        "{threshold_percent}%"
                    }
                }
            }
            p {
                class: "cache-settings__help",
                "Higher threshold means more exact matches required. Lower threshold allows more semantic similarity."
            }
        }
    }
}

/// Props for CacheTooltip component
#[derive(Props, Clone, PartialEq)]
pub struct CacheTooltipProps {
    /// The cached query text to display
    pub cached_query: String,
    /// Similarity score
    pub similarity: f32,
    /// Whether the tooltip is visible
    pub visible: Signal<bool>,
}

/// Tooltip showing the original cached query text.
#[component]
pub fn CacheTooltip(props: CacheTooltipProps) -> Element {
    let CacheTooltipProps {
        cached_query,
        similarity,
        visible,
    } = props;

    if !visible() {
        return rsx! {};
    }

    let similarity_percent = (similarity * 100.0).round() as u32;

    rsx! {
        div {
            class: "cache-tooltip",
            div {
                class: "cache-tooltip__header",
                span {
                    class: "cache-tooltip__label",
                    "Similar cached query"
                }
                span {
                    class: "cache-tooltip__similarity",
                    "{similarity_percent}% similar"
                }
            }
            pre {
                class: "cache-tooltip__query",
                {cached_query.to_string()}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_hit_info_default() {
        let info = CacheHitInfo {
            is_hit: false,
            similarity_score: None,
            cached_query: None,
        };
        assert!(!info.is_hit);
        assert!(info.similarity_score.is_none());
    }

    #[test]
    fn test_cache_hit_info_with_score() {
        let info = CacheHitInfo {
            is_hit: true,
            similarity_score: Some(0.94),
            cached_query: Some("SELECT * FROM users".to_string()),
        };
        assert!(info.is_hit);
        assert_eq!(info.similarity_score, Some(0.94));
        assert_eq!(info.cached_query, Some("SELECT * FROM users".to_string()));
    }

    #[test]
    fn test_cache_display_stats_default() {
        let stats = CacheDisplayStats::default();
        assert_eq!(stats.total_entries, 0);
        assert_eq!(stats.total_hits, 0);
        assert_eq!(stats.hit_rate, 0.0);
        assert!(!stats.is_warming);
    }

    #[test]
    fn test_threshold_constants() {
        assert_eq!(DEFAULT_SIMILARITY_THRESHOLD, 0.85);
        assert_eq!(MIN_SIMILARITY_THRESHOLD, 0.80);
        assert_eq!(MAX_SIMILARITY_THRESHOLD, 0.95);
    }
}
