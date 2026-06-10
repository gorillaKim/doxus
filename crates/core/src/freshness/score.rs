#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RetentionTier {
    Short,
    Mid,
    Long,
}

impl std::str::FromStr for RetentionTier {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "short" => RetentionTier::Short,
            "long" => RetentionTier::Long,
            _ => RetentionTier::Mid,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SensitivityMode {
    Strict,
    Normal,
    Relaxed,
}

impl std::str::FromStr for SensitivityMode {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "strict" => SensitivityMode::Strict,
            "relaxed" => SensitivityMode::Relaxed,
            _ => SensitivityMode::Normal,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FreshnessStatus {
    Fresh,
    Aging,
    Stale,
    Obsolete,
}

impl std::str::FromStr for FreshnessStatus {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "aging" => FreshnessStatus::Aging,
            "stale" => FreshnessStatus::Stale,
            "obsolete" => FreshnessStatus::Obsolete,
            _ => FreshnessStatus::Fresh,
        })
    }
}

impl FreshnessStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            FreshnessStatus::Fresh => "fresh",
            FreshnessStatus::Aging => "aging",
            FreshnessStatus::Stale => "stale",
            FreshnessStatus::Obsolete => "obsolete",
        }
    }
}

pub struct Thresholds {
    pub fresh: f64, // 기본 70.0
    pub aging: f64, // 기본 40.0
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            fresh: 70.0,
            aging: 40.0,
        }
    }
}

pub fn base_half_life(tier: RetentionTier) -> f64 {
    match tier {
        RetentionTier::Short => 45.0,
        RetentionTier::Mid => 90.0,
        RetentionTier::Long => 180.0,
    }
}

pub fn sensitivity_multiplier(mode: SensitivityMode) -> f64 {
    match mode {
        SensitivityMode::Strict => 0.5,
        SensitivityMode::Normal => 1.0,
        SensitivityMode::Relaxed => 1.5,
    }
}

pub fn calculate_freshness(
    days_since_update: f64,
    tier: RetentionTier,
    mode: SensitivityMode,
) -> f64 {
    let half_life = base_half_life(tier) * sensitivity_multiplier(mode);
    let lambda = (2.0_f64).ln() / half_life;
    (100.0 * (-lambda * days_since_update).exp()).clamp(0.0, 100.0)
}

pub fn score_to_status(score: f64, thresholds: &Thresholds) -> FreshnessStatus {
    if score >= thresholds.fresh {
        FreshnessStatus::Fresh
    } else if score >= thresholds.aging {
        FreshnessStatus::Aging
    } else {
        FreshnessStatus::Stale
    }
}

/// 소스 타입 → 기본 등급 매핑
pub fn default_tier_for_source(plugin_id: &str) -> RetentionTier {
    if plugin_id.contains("github") {
        RetentionTier::Short
    } else {
        RetentionTier::Mid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_freshness_decay() {
        // Normal mode, Mid tier (half-life = 90 days)
        let score_start = calculate_freshness(0.0, RetentionTier::Mid, SensitivityMode::Normal);
        assert!((score_start - 100.0).abs() < 0.001);

        let score_half = calculate_freshness(90.0, RetentionTier::Mid, SensitivityMode::Normal);
        assert!((score_half - 50.0).abs() < 0.001);

        let score_quarter = calculate_freshness(180.0, RetentionTier::Mid, SensitivityMode::Normal);
        assert!((score_quarter - 25.0).abs() < 0.001);
    }

    #[test]
    fn test_sensitivity_modes() {
        // Strict mode halves the half-life -> decays twice as fast
        let score_strict = calculate_freshness(45.0, RetentionTier::Mid, SensitivityMode::Strict);
        assert!((score_strict - 50.0).abs() < 0.001); // 90 * 0.5 = 45 days half-life

        // Relaxed mode multiplies half-life by 1.5 -> decays slower
        let score_relaxed =
            calculate_freshness(135.0, RetentionTier::Mid, SensitivityMode::Relaxed);
        assert!((score_relaxed - 50.0).abs() < 0.001); // 90 * 1.5 = 135 days half-life
    }

    #[test]
    fn test_retention_tiers() {
        let score_short = calculate_freshness(45.0, RetentionTier::Short, SensitivityMode::Normal);
        assert!((score_short - 50.0).abs() < 0.001);

        let score_long = calculate_freshness(180.0, RetentionTier::Long, SensitivityMode::Normal);
        assert!((score_long - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_score_to_status() {
        let thresholds = Thresholds::default(); // fresh: 70, aging: 40
        assert_eq!(score_to_status(80.0, &thresholds), FreshnessStatus::Fresh);
        assert_eq!(score_to_status(70.0, &thresholds), FreshnessStatus::Fresh);
        assert_eq!(score_to_status(69.9, &thresholds), FreshnessStatus::Aging);
        assert_eq!(score_to_status(40.0, &thresholds), FreshnessStatus::Aging);
        assert_eq!(score_to_status(39.9, &thresholds), FreshnessStatus::Stale);
        assert_eq!(score_to_status(0.0, &thresholds), FreshnessStatus::Stale);
    }

    #[test]
    fn test_default_tier() {
        assert_eq!(
            default_tier_for_source("com.doxus.github"),
            RetentionTier::Short
        );
        assert_eq!(
            default_tier_for_source("com.doxus.confluence"),
            RetentionTier::Mid
        );
        assert_eq!(
            default_tier_for_source("com.doxus.obsidian"),
            RetentionTier::Mid
        );
    }
}
