use crate::SkillMetadata;
use codex_protocol::openai_models::ModelPreset;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashSet;

pub const MODEL_PROFILES_TOML_FILE: &str = "model-profiles.toml";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProfilesToml {
    #[serde(default = "default_model_profiles_version")]
    pub version: u32,
    #[serde(default)]
    pub generated_at: Option<String>,
    #[serde(default)]
    pub models: Vec<ModelProfileEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProfileEntry {
    pub model: String,
    #[serde(default)]
    pub task_tags: Vec<String>,
    #[serde(default)]
    pub strengths: Vec<String>,
    #[serde(default)]
    pub cost_tier: Option<String>,
    #[serde(default)]
    pub speed_tier: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SkillModelRouteDecision {
    pub(crate) requested_model: Option<String>,
    pub(crate) route_reason: Option<String>,
    pub(crate) fallback_reason: Option<String>,
}

const fn default_model_profiles_version() -> u32 {
    1
}

pub fn model_profiles_path(codex_home: &AbsolutePathBuf) -> AbsolutePathBuf {
    codex_home.join(MODEL_PROFILES_TOML_FILE)
}

pub(crate) async fn load_model_profiles(
    codex_home: &AbsolutePathBuf,
) -> Result<Option<ModelProfilesToml>, String> {
    let path = model_profiles_path(codex_home);
    let contents = match tokio::fs::read_to_string(path.as_path()).await {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(format!(
                "failed to read model profiles at {}: {err}",
                path.display()
            ));
        }
    };
    toml::from_str(&contents).map(Some).map_err(|err| {
        format!(
            "failed to parse model profiles at {}: {err}",
            path.display()
        )
    })
}

pub(crate) fn resolve_skill_routed_model(
    mentioned_skills: &[SkillMetadata],
    available_models: &[ModelPreset],
    profiles: &ModelProfilesToml,
) -> SkillModelRouteDecision {
    let route_tags = collect_route_tags(mentioned_skills);
    if route_tags.is_empty() {
        return SkillModelRouteDecision::default();
    }

    let available_names: HashSet<&str> = available_models
        .iter()
        .map(|preset| preset.model.as_str())
        .collect();
    let available_skill_names = mentioned_skills
        .iter()
        .filter(|skill| {
            skill
                .routing
                .as_ref()
                .is_some_and(|routing| !routing.task_tags.is_empty())
        })
        .map(|skill| skill.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    let mut selected: Option<(&ModelProfileEntry, usize)> = None;
    for profile in &profiles.models {
        if !model_name_is_available(&profile.model, &available_names) {
            continue;
        }
        let score = route_tags
            .iter()
            .filter(|task_tag| {
                profile
                    .task_tags
                    .iter()
                    .any(|candidate| candidate == *task_tag)
            })
            .count();
        if score == 0 {
            continue;
        }
        if selected
            .as_ref()
            .is_none_or(|(_, best_score)| score > *best_score)
        {
            selected = Some((profile, score));
        }
    }

    match selected {
        Some((profile, score)) => SkillModelRouteDecision {
            requested_model: Some(profile.model.clone()),
            route_reason: Some(format!(
                "matched skill task tags [{}] from {available_skill_names} to profile `{}` with score {score}",
                route_tags.join(", "),
                profile.model
            )),
            fallback_reason: None,
        },
        None => SkillModelRouteDecision {
            requested_model: None,
            route_reason: None,
            fallback_reason: Some(format!(
                "no available model profile matched skill task tags [{}] from {available_skill_names}",
                route_tags.join(", ")
            )),
        },
    }
}

/// Returns true when `model_name` appears in the available set, either as an
/// exact match or via namespace-aware fallback (e.g. `cx/gpt-5.4` matches
/// when only `gpt-5.4` is available, and vice‑versa).
fn model_name_is_available(model_name: &str, available: &HashSet<&str>) -> bool {
    if available.contains(model_name) {
        return true;
    }
    // Named profile model → check bare suffix against available set.
    if let Some((_ns, suffix)) = model_name.split_once('/') {
        if available.contains(suffix) {
            return true;
        }
    }
    // Bare profile model → check whether any available entry ends with /{name}.
    if !model_name.contains('/') {
        let suffixed = format!("/{model_name}");
        return available.iter().any(|name| name.ends_with(&suffixed));
    }
    false
}

fn collect_route_tags(mentioned_skills: &[SkillMetadata]) -> Vec<String> {
    let mut tags = Vec::new();
    let mut seen = HashSet::new();
    for skill in mentioned_skills {
        let Some(routing) = skill.routing.as_ref() else {
            continue;
        };
        for task_tag in &routing.task_tags {
            if seen.insert(task_tag.clone()) {
                tags.push(task_tag.clone());
            }
        }
    }
    tags
}

pub fn build_suggested_model_profiles(
    available_models: &[ModelPreset],
    generated_at: Option<String>,
) -> ModelProfilesToml {
    ModelProfilesToml {
        version: default_model_profiles_version(),
        generated_at,
        models: available_models
            .iter()
            .map(suggest_profile_for_model)
            .collect(),
    }
}

fn suggest_profile_for_model(preset: &ModelPreset) -> ModelProfileEntry {
    let lowercase_name = preset.model.to_ascii_lowercase();
    let lowercase_description = preset.description.to_ascii_lowercase();
    let mut task_tags = Vec::new();
    let mut strengths = Vec::new();

    if lowercase_name.contains("mini")
        || lowercase_name.contains("nano")
        || lowercase_description.contains("fast")
        || lowercase_description.contains("cheap")
    {
        task_tags.push("fast-cheap".to_string());
        task_tags.push("classification".to_string());
        strengths.push("Low-cost candidate for lightweight delegation.".to_string());
    }

    if lowercase_name.contains("5.5")
        || lowercase_description.contains("complex")
        || lowercase_description.contains("reasoning")
    {
        task_tags.push("deep-reasoning".to_string());
        task_tags.push("research".to_string());
        task_tags.push("review".to_string());
        strengths.push("Better fit for deep reasoning, research, and review tasks.".to_string());
    }

    if task_tags.is_empty() {
        task_tags.push("coding".to_string());
        task_tags.push("review".to_string());
        strengths.push("General-purpose coding and review default.".to_string());
    } else if !task_tags.iter().any(|tag| tag == "coding") {
        task_tags.push("coding".to_string());
    }

    ModelProfileEntry {
        model: preset.model.clone(),
        task_tags,
        strengths,
        cost_tier: Some(
            if lowercase_name.contains("mini") || lowercase_name.contains("nano") {
                "low".to_string()
            } else if lowercase_name.contains("5.5") {
                "high".to_string()
            } else {
                "medium".to_string()
            },
        ),
        speed_tier: Some(
            if lowercase_name.contains("mini") || lowercase_name.contains("nano") {
                "fast".to_string()
            } else if lowercase_name.contains("5.5") {
                "slow".to_string()
            } else {
                "medium".to_string()
            },
        ),
        notes: Some(format!("{} {}", preset.display_name, preset.description)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::model::SkillRouting;
    use codex_protocol::protocol::SkillScope;
    use codex_utils_absolute_path::AbsolutePathBuf;
    use pretty_assertions::assert_eq;

    fn skill(name: &str, task_tags: &[&str]) -> SkillMetadata {
        SkillMetadata {
            name: name.to_string(),
            description: "desc".to_string(),
            short_description: None,
            interface: None,
            dependencies: None,
            policy: None,
            routing: Some(SkillRouting {
                task_tags: task_tags.iter().map(|tag| (*tag).to_string()).collect(),
            }),
            path_to_skills_md: AbsolutePathBuf::from_absolute_path(
                std::env::temp_dir().join(format!("{name}/SKILL.md")),
            )
            .expect("absolute skill path"),
            scope: SkillScope::User,
            plugin_id: None,
        }
    }

    fn preset(model: &str) -> ModelPreset {
        ModelPreset {
            id: model.to_string(),
            model: model.to_string(),
            display_name: model.to_string(),
            description: "desc".to_string(),
            default_reasoning_effort: codex_protocol::openai_models::ReasoningEffort::Medium,
            supported_reasoning_efforts: Vec::new(),
            supports_personality: true,
            additional_speed_tiers: Vec::new(),
            service_tiers: Vec::new(),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            availability_nux: None,
            supported_in_api: true,
            input_modalities: Vec::new(),
        }
    }

    #[test]
    fn resolve_skill_routed_model_uses_best_matching_profile() {
        let profiles = ModelProfilesToml {
            version: 1,
            generated_at: None,
            models: vec![
                ModelProfileEntry {
                    model: "gpt-5.4-mini".to_string(),
                    task_tags: vec!["fast-cheap".to_string(), "classification".to_string()],
                    strengths: Vec::new(),
                    cost_tier: None,
                    speed_tier: None,
                    notes: None,
                },
                ModelProfileEntry {
                    model: "gpt-5.5".to_string(),
                    task_tags: vec!["research".to_string(), "deep-reasoning".to_string()],
                    strengths: Vec::new(),
                    cost_tier: None,
                    speed_tier: None,
                    notes: None,
                },
            ],
        };

        let decision = resolve_skill_routed_model(
            &[skill("research-skill", &["research", "deep-reasoning"])],
            &[preset("gpt-5.4-mini"), preset("gpt-5.5")],
            &profiles,
        );

        assert_eq!(decision.requested_model.as_deref(), Some("gpt-5.5"));
        assert_eq!(decision.fallback_reason, None);
        assert!(
            decision
                .route_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("research-skill"))
        );
    }

    #[test]
    fn resolve_skill_routed_model_reports_fallback_when_no_profile_matches() {
        let profiles = ModelProfilesToml {
            version: 1,
            generated_at: None,
            models: vec![ModelProfileEntry {
                model: "gpt-5.4-mini".to_string(),
                task_tags: vec!["fast-cheap".to_string()],
                strengths: Vec::new(),
                cost_tier: None,
                speed_tier: None,
                notes: None,
            }],
        };

        let decision = resolve_skill_routed_model(
            &[skill("review-skill", &["review"])],
            &[preset("gpt-5.4-mini")],
            &profiles,
        );

        assert_eq!(decision.requested_model, None);
        assert!(
            decision
                .fallback_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("review"))
        );
    }

    #[test]
    fn resolve_skill_routed_model_matches_namespaced_profile_to_bare_available() {
        // Profile uses namespaced ID (e.g. "cx/gpt-5.4") but available list
        // only has the bare name ("gpt-5.4").
        let profiles = ModelProfilesToml {
            version: 1,
            generated_at: None,
            models: vec![ModelProfileEntry {
                model: "cx/gpt-5.4".to_string(),
                task_tags: vec!["coding".to_string()],
                strengths: Vec::new(),
                cost_tier: None,
                speed_tier: None,
                notes: None,
            }],
        };

        let decision = resolve_skill_routed_model(
            &[skill("code-skill", &["coding"])],
            &[preset("gpt-5.4")],
            &profiles,
        );

        assert_eq!(decision.requested_model.as_deref(), Some("cx/gpt-5.4"));
        assert_eq!(decision.fallback_reason, None);
    }

    #[test]
    fn resolve_skill_routed_model_matches_bare_profile_to_namespaced_available() {
        // Profile uses bare name ("gpt-5.4") but available list has the
        // namespaced version ("cx/gpt-5.4").
        let profiles = ModelProfilesToml {
            version: 1,
            generated_at: None,
            models: vec![ModelProfileEntry {
                model: "gpt-5.4".to_string(),
                task_tags: vec!["coding".to_string()],
                strengths: Vec::new(),
                cost_tier: None,
                speed_tier: None,
                notes: None,
            }],
        };

        let decision = resolve_skill_routed_model(
            &[skill("code-skill", &["coding"])],
            &[preset("cx/gpt-5.4")],
            &profiles,
        );

        assert_eq!(decision.requested_model.as_deref(), Some("gpt-5.4"));
        assert_eq!(decision.fallback_reason, None);
    }
}
