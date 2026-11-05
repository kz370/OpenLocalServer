//! §166: the user's own compose builder presets ("My presets").
//!
//! Stored in the `docker.compose.custom_presets` setting as `ServicePreset` JSON. Every
//! field is checked on save, because a preset is rendered into compose files: an image,
//! tag, port, environment key or volume that is not a plain value is refused.

use std::collections::BTreeSet;

use super::builder::{presets, valid_image, valid_tag, ServicePreset};
use super::compose::valid_service;
use crate::error::CoreError;

pub const SETTING_CUSTOM_PRESETS: &str = "docker.compose.custom_presets";
/// Category shown for presets that do not name one.
pub const MY_PRESETS: &str = "My presets";
const MAX_PRESETS: usize = 200;

fn bad(cause: impl Into<String>, fix: &str) -> CoreError {
    CoreError::failed_fix("The preset was not saved.", cause, fix)
}

/// `custom-<slug of name>`, so a user preset can never take a built-in id.
pub fn preset_id(name: &str) -> String {
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    format!("custom-{}", if slug.is_empty() { "preset" } else { &slug })
}

fn plain_text(s: &str, max: usize) -> bool {
    s.len() <= max && !s.chars().any(|c| c.is_control())
}

fn valid_env_key(k: &str) -> bool {
    let mut chars = k.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && k.len() <= 128
}

/// A volume source: a named volume (`data`) or a path inside the project (`./`, `./data`).
fn valid_volume_source(s: &str) -> bool {
    if s == "." || s == "./" {
        return true;
    }
    if let Some(rest) = s.strip_prefix("./") {
        return !rest
            .split('/')
            .any(|p| p == ".." || p.is_empty() && !rest.ends_with('/'))
            && rest
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/'));
    }
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

fn valid_target(t: &str) -> bool {
    t.starts_with('/')
        && t.len() <= 255
        && !t.split('/').any(|p| p == "..")
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/'))
}

/// Checks and normalises a preset before it is stored.
pub fn validate(mut p: ServicePreset) -> Result<ServicePreset, CoreError> {
    p.name = p.name.trim().to_string();
    if p.name.is_empty() || !plain_text(&p.name, 60) {
        return Err(bad(
            "The preset needs a short name.",
            "Type a name such as \"Company API\".",
        ));
    }
    if p.id.trim().is_empty() {
        p.id = preset_id(&p.name);
    }
    if !p.id.starts_with("custom-") {
        return Err(bad(
            format!("\"{}\" is the id of a built-in preset.", p.id),
            "Save it under a new name instead.",
        ));
    }
    p.category = p.category.trim().to_string();
    if p.category.is_empty() {
        p.category = MY_PRESETS.into();
    }
    if !plain_text(&p.category, 40) || !plain_text(&p.description, 200) {
        return Err(bad(
            "The category or description is too long.",
            "Shorten it.",
        ));
    }
    p.image = p.image.trim().to_string();
    // The version lives in `tags`; the name's last part must not carry one (a registry
    // host may still have a port, as in localhost:5000/app).
    let last = p.image.rsplit('/').next().unwrap_or_default();
    if !valid_image(&p.image) || last.contains(':') || last.contains('@') {
        return Err(bad(
            format!("\"{}\" is not an image name.", p.image),
            "Type the image without its version, for example `ghcr.io/acme/api`.",
        ));
    }
    let tags: Vec<String> = p
        .tags
        .iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    p.tags = tags
        .into_iter()
        .filter(|t| seen.insert(t.clone()))
        .collect();
    if p.tags.is_empty() {
        return Err(bad(
            "Add at least one version (tag).",
            "For example `1.4` or `2024-06`.",
        ));
    }
    if let Some(t) = p.tags.iter().find(|t| !valid_tag(t)) {
        return Err(bad(
            format!("\"{t}\" is not a version tag."),
            "Use letters, digits, `.`, `_` and `-`.",
        ));
    }
    p.tag = p.tags[0].clone();
    p.default_service = p.default_service.trim().to_string();
    if p.default_service.is_empty() {
        p.default_service = "app".into();
    }
    if !valid_service(&p.default_service) {
        return Err(bad(
            format!("\"{}\" is not a service name.", p.default_service),
            "Use letters, digits, `_`, `.` and `-`.",
        ));
    }
    let mut container_ports = BTreeSet::new();
    for port in &p.ports {
        if port.container == 0 || port.default_host == 0 || !container_ports.insert(port.container)
        {
            return Err(bad(
                "Each port needs a container port and a port on this computer (1–65535), and container ports must not repeat.",
                "Fix the ports list.",
            ));
        }
        if !plain_text(&port.label, 40) {
            return Err(bad("A port label is too long.", "Shorten it."));
        }
    }
    let mut keys = BTreeSet::new();
    for e in &p.env {
        if !valid_env_key(&e.key) || !keys.insert(e.key.clone()) {
            return Err(bad(
                format!(
                    "\"{}\" is not a usable variable name, or it is listed twice.",
                    e.key
                ),
                "Use letters, digits and `_`, starting with a letter.",
            ));
        }
        if let super::builder::EnvValue::Literal { value } = &e.value {
            if !plain_text(value, 1000) {
                return Err(bad(
                    format!("The value of {} is too long or has line breaks.", e.key),
                    "Shorten it.",
                ));
            }
        }
    }
    for v in &p.volumes {
        if !valid_volume_source(&v.source) || !valid_target(&v.target) {
            return Err(bad(
                format!("Volume \"{}:{}\" is not valid.", v.source, v.target),
                "Use a volume name (data) or a folder inside the project (./data), and an absolute path inside the container (/var/lib/app).",
            ));
        }
    }
    if let Some(cmd) = &p.command {
        if cmd.trim().is_empty() {
            p.command = None;
        } else if !plain_text(cmd, 500) {
            return Err(bad(
                "The command is too long or has line breaks.",
                "Put it on one line.",
            ));
        }
    }
    if let Some(wd) = &p.working_dir {
        if wd.trim().is_empty() {
            p.working_dir = None;
        } else if !valid_target(wd) {
            return Err(bad(
                "The working folder must be an absolute path inside the container.",
                "For example /app.",
            ));
        }
    }
    p.custom = true;
    Ok(p)
}

/// Stored presets; anything that no longer passes the checks is skipped, not fatal.
pub fn load(value: Option<&serde_json::Value>) -> Vec<ServicePreset> {
    value
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| serde_json::from_value::<ServicePreset>(v.clone()).ok())
                .filter_map(|p| validate(p).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Built-in presets followed by the user's.
pub fn catalog(custom: &[ServicePreset]) -> Vec<ServicePreset> {
    let mut all = presets();
    all.extend(custom.iter().cloned());
    all
}

/// Adds or replaces (same id) a preset. `previous_id` renames: the old entry goes.
pub fn upsert(
    mut list: Vec<ServicePreset>,
    preset: ServicePreset,
    previous_id: Option<&str>,
) -> Result<(Vec<ServicePreset>, ServicePreset), CoreError> {
    let preset = validate(preset)?;
    if presets().iter().any(|b| b.id == preset.id) {
        return Err(bad(
            "That id belongs to a built-in preset.",
            "Use another name.",
        ));
    }
    let replacing = previous_id.unwrap_or(&preset.id).to_string();
    if list.iter().any(|p| p.id == preset.id && p.id != replacing) {
        return Err(bad(
            format!("A preset named \"{}\" already exists.", preset.name),
            "Use another name, or edit that preset.",
        ));
    }
    list.retain(|p| p.id != replacing);
    if list.len() >= MAX_PRESETS {
        return Err(bad(
            "You have reached 200 presets.",
            "Delete presets you no longer use.",
        ));
    }
    list.push(preset.clone());
    list.sort_by(|a, b| (&a.category, &a.name).cmp(&(&b.category, &b.name)));
    Ok((list, preset))
}

impl crate::app::Inner {
    pub fn docker_custom_presets(&self) -> Vec<ServicePreset> {
        load(self.settings.lock().unwrap().get(SETTING_CUSTOM_PRESETS))
    }

    fn store_presets(&self, list: &[ServicePreset]) -> Result<(), CoreError> {
        let value = serde_json::to_value(list)?;
        self.settings
            .lock()
            .unwrap()
            .set(SETTING_CUSTOM_PRESETS, value)
    }

    pub fn save_docker_preset(
        &self,
        preset: ServicePreset,
        previous_id: Option<&str>,
    ) -> Result<ServicePreset, CoreError> {
        let (list, saved) = upsert(self.docker_custom_presets(), preset, previous_id)?;
        self.store_presets(&list)?;
        Ok(saved)
    }

    pub fn delete_docker_preset(&self, id: &str) -> Result<(), CoreError> {
        let mut list = self.docker_custom_presets();
        let before = list.len();
        list.retain(|p| p.id != id);
        if list.len() == before {
            return Err(CoreError::failed_fix(
                "That preset was not found.",
                format!(
                    "No preset of yours has the id \"{id}\"; built-in presets cannot be deleted."
                ),
                "Refresh the builder and try again.",
            ));
        }
        self.store_presets(&list)
    }
}

#[cfg(test)]
mod tests {
    use super::super::builder::{EnvValue, PresetEnv, PresetPort, PresetVolume};
    use super::*;

    fn sample() -> ServicePreset {
        ServicePreset {
            id: String::new(),
            name: "Company API".into(),
            category: String::new(),
            description: "Our backend".into(),
            image: "ghcr.io/acme/api".into(),
            tag: String::new(),
            tags: vec!["2.4".into(), " 2.3 ".into(), "2.4".into()],
            default_service: "api".into(),
            ports: vec![PresetPort {
                container: 8080,
                default_host: 8090,
                label: "HTTP".into(),
                web: true,
            }],
            env: vec![
                PresetEnv {
                    key: "APP_ENV".into(),
                    value: EnvValue::Literal {
                        value: "local".into(),
                    },
                },
                PresetEnv {
                    key: "API_KEY".into(),
                    value: EnvValue::Secret,
                },
            ],
            volumes: vec![PresetVolume {
                source: "./storage".into(),
                target: "/app/storage".into(),
                read_only: false,
            }],
            command: Some("  ".into()),
            working_dir: Some("/app".into()),
            custom: false,
        }
    }

    #[test]
    fn validate_normalises_a_good_preset() {
        let p = validate(sample()).unwrap();
        assert_eq!(p.id, "custom-company-api");
        assert_eq!(p.category, MY_PRESETS);
        assert_eq!(p.tags, vec!["2.4", "2.3"], "trimmed and de-duplicated");
        assert_eq!(p.tag, "2.4");
        assert!(p.custom && p.command.is_none());
    }

    #[test]
    fn validate_refuses_anything_that_is_not_plain() {
        let check = |f: &dyn Fn(&mut ServicePreset)| {
            let mut p = sample();
            f(&mut p);
            validate(p).is_err()
        };
        assert!(check(&|p| p.name = " ".into()));
        assert!(
            check(&|p| p.id = "mariadb".into()),
            "cannot take a built-in id"
        );
        assert!(check(&|p| p.image = "bad image".into()));
        assert!(
            check(&|p| p.image = "nginx:1.28".into()),
            "version goes in tags"
        );
        assert!(
            !check(&|p| p.image = "localhost:5000/acme/api".into()),
            "registry port is fine"
        );
        assert!(check(&|p| p.tags = vec![]));
        assert!(check(&|p| p.tags = vec!["1.0\nservices:".into()]));
        assert!(check(&|p| p.default_service = "a b".into()));
        assert!(check(&|p| p.ports[0].container = 0));
        assert!(check(&|p| p.env[0].key = "1BAD".into()));
        assert!(
            check(&|p| p.env[1].key = "APP_ENV".into()),
            "duplicate keys"
        );
        assert!(check(&|p| p.env[0].value = EnvValue::Literal {
            value: "a\nb".into()
        }));
        assert!(check(&|p| p.volumes[0].source = "../../etc".into()));
        assert!(check(&|p| p.volumes[0].source = "./../x".into()));
        assert!(check(&|p| p.volumes[0].target = "relative".into()));
        assert!(check(&|p| p.command = Some("a\nb".into())));
        assert!(check(&|p| p.working_dir = Some("app".into())));
    }

    #[test]
    fn upsert_adds_renames_and_refuses_clashes() {
        let (list, saved) = upsert(vec![], sample(), None).unwrap();
        assert_eq!(list.len(), 1);
        // Same name again (new preset): refused.
        assert!(upsert(list.clone(), sample(), Some("custom-other")).is_err());
        // Edit in place.
        let mut edited = saved.clone();
        edited.description = "Updated".into();
        let (list, _) = upsert(list, edited, Some(&saved.id)).unwrap();
        assert_eq!((list.len(), list[0].description.as_str()), (1, "Updated"));
        // Rename: the old id goes.
        let mut renamed = list[0].clone();
        renamed.name = "Billing API".into();
        renamed.id = String::new();
        let (list, r) = upsert(list, renamed, Some("custom-company-api")).unwrap();
        assert_eq!((list.len(), r.id.as_str()), (1, "custom-billing-api"));
    }

    #[test]
    fn custom_presets_plan_like_built_ins() {
        let tmp = tempfile::tempdir().unwrap();
        let p = validate(sample()).unwrap();
        let spec = super::super::builder::BuildSpec {
            dir: tmp.path().display().to_string(),
            name: None,
            services: vec![super::super::builder::BuildService {
                preset: p.id.clone(),
                name: "api".into(),
                tag: Some("2.3".into()),
                host_ports: vec![],
                image: None,
                container_port: None,
                env: Default::default(),
            }],
        };
        let plan = super::super::builder::plan_with(&spec, &catalog(&[p]), &|_| true).unwrap();
        assert!(
            plan.yaml.contains("\"ghcr.io/acme/api:2.3\""),
            "{}",
            plan.yaml
        );
        assert!(plan.yaml.contains("\"127.0.0.1:8090:8080\""));
        assert!(plan.yaml.contains("${OLS_SECRET_API_API_KEY:?"));
        assert!(plan.yaml.contains("\"./storage:/app/storage\""));
        assert_eq!(plan.secrets, vec!["OLS_SECRET_API_API_KEY"]);
        // Built-in plan does not know it.
        assert!(super::super::builder::plan(&spec, &|_| true).is_err());
    }

    #[test]
    fn stored_junk_is_skipped() {
        let v = serde_json::json!([serde_json::to_value(validate(sample()).unwrap()).unwrap(), {"id": "x"}, 5]);
        assert_eq!(load(Some(&v)).len(), 1);
        assert!(load(None).is_empty());
    }
}
