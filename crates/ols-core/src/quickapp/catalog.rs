//! Quick App catalog (§80, §82, §87–88, §139): where definitions come from and whether
//! they may run.
//!
//! * **Built-in** — embedded in the binary. Trusted.
//! * **Local**    — files the user created, duplicated or edited (`quick-apps/local/`). Trusted
//!                  by the user. A local file with a built-in's id *overrides* it; deleting
//!                  the file restores the original.
//! * **Imported** — copied in from a file, folder or Git repository (`quick-apps/imported/`).
//!                  Untrusted until the user approves the source (§88).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::schema::{self, QuickApp};
use crate::error::CoreError;
use crate::paths::AppPaths;

const BUILTIN: &[(&str, &str)] = &[
    ("laravel", include_str!("../../catalog/quick-apps/laravel.yaml")),
    ("symfony", include_str!("../../catalog/quick-apps/symfony.yaml")),
    ("wordpress", include_str!("../../catalog/quick-apps/wordpress.yaml")),
    ("react-vite", include_str!("../../catalog/quick-apps/react-vite.yaml")),
    ("vue-vite", include_str!("../../catalog/quick-apps/vue-vite.yaml")),
    ("nextjs", include_str!("../../catalog/quick-apps/nextjs.yaml")),
    ("express-api", include_str!("../../catalog/quick-apps/express-api.yaml")),
    ("fastapi", include_str!("../../catalog/quick-apps/fastapi.yaml")),
    ("django", include_str!("../../catalog/quick-apps/django.yaml")),
    ("plain-php", include_str!("../../catalog/quick-apps/plain-php.yaml")),
    ("static-html", include_str!("../../catalog/quick-apps/static-html.yaml")),
    ("custom-app", include_str!("../../catalog/quick-apps/custom-app.yaml")),
    ("reverse-proxy", include_str!("../../catalog/quick-apps/reverse-proxy.yaml")),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntrySource {
    Builtin,
    Local,
    Imported,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub source: EntrySource,
    pub trusted: bool,
    pub favorite: bool,
    /// Where an imported definition came from (its trust unit).
    pub origin: Option<String>,
    /// A local file shadowing a built-in of the same id.
    pub overrides_builtin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryDetail {
    pub view: EntryView,
    pub app: QuickApp,
    pub yaml: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Meta {
    #[serde(default)]
    favorites: BTreeSet<String>,
    #[serde(default)]
    trusted_sources: BTreeSet<String>,
    /// imported file stem (id) → origin string.
    #[serde(default)]
    origins: BTreeMap<String, String>,
}

pub struct QuickCatalog {
    root: PathBuf,
    meta_file: PathBuf,
    meta: Meta,
}

struct Loaded {
    app: QuickApp,
    yaml: String,
    source: EntrySource,
    origin: Option<String>,
    overrides_builtin: bool,
}

fn err(msg: impl Into<String>) -> CoreError {
    CoreError::QuickAppError(msg.into())
}

impl QuickCatalog {
    pub fn load(paths: &AppPaths) -> Result<Self, CoreError> {
        paths.ensure_dirs()?;
        let root = paths.quick_apps_dir();
        std::fs::create_dir_all(root.join("local"))?;
        std::fs::create_dir_all(root.join("imported"))?;
        let meta_file = paths.data_dir().join("quick_apps_meta.json");
        let meta = std::fs::read_to_string(&meta_file).ok().and_then(|r| serde_json::from_str(&r).ok()).unwrap_or_default();
        Ok(Self { root, meta_file, meta })
    }

    fn local_dir(&self) -> PathBuf {
        self.root.join("local")
    }
    fn imported_dir(&self) -> PathBuf {
        self.root.join("imported")
    }

    fn persist(&self) -> Result<(), CoreError> {
        let raw = serde_json::to_string_pretty(&self.meta)?;
        let tmp = self.meta_file.with_extension("json.tmp");
        std::fs::write(&tmp, raw)?;
        std::fs::rename(&tmp, &self.meta_file)?;
        Ok(())
    }

    fn read_dir_defs(dir: &Path) -> Vec<(QuickApp, String, PathBuf)> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else { return out };
        for e in entries.flatten() {
            let path = e.path();
            let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("");
            if !(ext == "yaml" || ext == "yml") {
                continue;
            }
            let Ok(yaml) = std::fs::read_to_string(&path) else { continue };
            // A broken file is skipped, not fatal — one bad import mustn't hide the catalog.
            if let Ok(app) = schema::parse(&yaml) {
                out.push((app, yaml, path));
            }
        }
        out
    }

    fn all(&self) -> Vec<Loaded> {
        let mut by_id: BTreeMap<String, Loaded> = BTreeMap::new();
        for (_, yaml) in BUILTIN {
            if let Ok(app) = schema::parse(yaml) {
                by_id.insert(app.id.clone(), Loaded { app, yaml: yaml.to_string(), source: EntrySource::Builtin, origin: None, overrides_builtin: false });
            }
        }
        // Imported next, so that a local file (trusted by the user) can override either.
        if let Ok(dirs) = std::fs::read_dir(self.imported_dir()) {
            for d in dirs.flatten() {
                let origin = self.meta.origins.get(&d.file_name().to_string_lossy().to_string()).cloned();
                for (app, yaml, _) in Self::read_dir_defs(&d.path()) {
                    // An import must not silently replace a built-in or local recipe.
                    by_id.entry(app.id.clone()).or_insert(Loaded {
                        app,
                        yaml,
                        source: EntrySource::Imported,
                        origin: origin.clone(),
                        overrides_builtin: false,
                    });
                }
            }
        }
        for (app, yaml, _) in Self::read_dir_defs(&self.local_dir()) {
            let overrides = by_id.get(&app.id).is_some_and(|l| l.source == EntrySource::Builtin);
            by_id.insert(app.id.clone(), Loaded { app, yaml, source: EntrySource::Local, origin: None, overrides_builtin: overrides });
        }
        by_id.into_values().collect()
    }

    fn view(&self, l: &Loaded) -> EntryView {
        let trusted = match l.source {
            EntrySource::Builtin | EntrySource::Local => true,
            EntrySource::Imported => l.origin.as_ref().is_some_and(|o| self.meta.trusted_sources.contains(o)),
        };
        EntryView {
            id: l.app.id.clone(),
            name: l.app.name.clone(),
            description: l.app.description.clone(),
            category: l.app.category.clone(),
            source: l.source,
            trusted,
            favorite: self.meta.favorites.contains(&l.app.id),
            origin: l.origin.clone(),
            overrides_builtin: l.overrides_builtin,
        }
    }

    pub fn list(&self) -> Vec<EntryView> {
        self.all().iter().map(|l| self.view(l)).collect()
    }

    pub fn get(&self, id: &str) -> Result<EntryDetail, CoreError> {
        let loaded = self.all().into_iter().find(|l| l.app.id == id).ok_or_else(|| err(format!("no Quick App with id \"{id}\"")))?;
        Ok(EntryDetail { view: self.view(&loaded), app: loaded.app, yaml: loaded.yaml })
    }

    /// Creates or replaces a **local** definition from YAML (Create / Edit, §80).
    pub fn save(&mut self, yaml: &str) -> Result<EntryDetail, CoreError> {
        let app = schema::parse(yaml).map_err(err)?;
        // Don't let an edit quietly overwrite an imported recipe of the same id.
        if self.all().iter().any(|l| l.app.id == app.id && l.source == EntrySource::Imported) {
            return Err(err(format!("\"{}\" is an imported Quick App. Duplicate it under a new id to edit it.", app.id)));
        }
        std::fs::write(self.local_dir().join(format!("{}.yaml", app.id)), yaml)?;
        self.get(&app.id)
    }

    pub fn duplicate(&mut self, id: &str, new_id: &str, new_name: &str) -> Result<EntryDetail, CoreError> {
        let source = self.get(id)?;
        if !schema::valid_id(new_id) {
            return Err(err(format!("id \"{new_id}\" must be lowercase letters, digits and dashes")));
        }
        if self.all().iter().any(|l| l.app.id == new_id) {
            return Err(err(format!("\"{new_id}\" already exists")));
        }
        let mut app = source.app;
        app.id = new_id.to_string();
        app.name = new_name.to_string();
        let yaml = schema::to_yaml(&app).map_err(err)?;
        self.save(&yaml)
    }

    /// Deletes a local or imported definition. Built-ins can't be deleted (a local override can).
    pub fn delete(&mut self, id: &str) -> Result<(), CoreError> {
        let mut removed = false;
        let local = self.local_dir().join(format!("{id}.yaml"));
        if local.is_file() {
            std::fs::remove_file(local)?;
            removed = true;
        }
        if let Ok(dirs) = std::fs::read_dir(self.imported_dir()) {
            for d in dirs.flatten() {
                for (app, _, path) in Self::read_dir_defs(&d.path()) {
                    if app.id == id {
                        std::fs::remove_file(path)?;
                        removed = true;
                    }
                }
            }
        }
        if !removed {
            return Err(err("built-in Quick Apps can't be deleted (duplicate one to make your own copy)"));
        }
        self.meta.favorites.remove(id);
        self.persist()
    }

    pub fn set_favorite(&mut self, id: &str, favorite: bool) -> Result<(), CoreError> {
        if favorite {
            self.meta.favorites.insert(id.to_string());
        } else {
            self.meta.favorites.remove(id);
        }
        self.persist()
    }

    pub fn export(&self, id: &str, dest: &str) -> Result<(), CoreError> {
        let detail = self.get(id)?;
        std::fs::write(dest, detail.yaml)?;
        Ok(())
    }

    /// §87: import from a local file, a local folder, or a Git repository URL. Imported
    /// definitions are **untrusted** until the source is approved (§88, §139).
    pub fn import(&mut self, source: &str) -> Result<Vec<String>, CoreError> {
        let is_git = source.starts_with("https://") || source.starts_with("http://") || source.starts_with("git@");
        // A name for this source's folder; also its trust unit.
        let folder_name = crate::domain::slugify(source.rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap_or("import"));
        let folder_name = if folder_name.is_empty() { "import".to_string() } else { folder_name };
        let dest = self.imported_dir().join(&folder_name);
        std::fs::create_dir_all(&dest)?;

        let mut ids = Vec::new();
        let mut copy_defs = |from: &Path, dest: &Path| -> Result<(), CoreError> {
            let defs = Self::read_dir_defs(from);
            for (app, yaml, _) in defs {
                std::fs::write(dest.join(format!("{}.yaml", app.id)), yaml)?;
                ids.push(app.id);
            }
            Ok(())
        };

        if is_git {
            let clone_dir = std::env::temp_dir().join(format!("ols-import-{}-{}", folder_name, crate::ca::unix_now()));
            let out = crate::exec::run_capture(
                Path::new("git"),
                &["clone".into(), "--depth".into(), "1".into(), source.into(), clone_dir.display().to_string()],
                None,
                &[],
                std::time::Duration::from_secs(120),
            );
            if !out.success() {
                return Err(err(format!("git clone failed: {}", out.combined())));
            }
            let result = copy_defs(&clone_dir, &dest).and_then(|_| {
                // Catalog repos keep recipes under quick-apps/.
                copy_defs(&clone_dir.join("quick-apps"), &dest)
            });
            let _ = std::fs::remove_dir_all(&clone_dir);
            result?;
        } else {
            let path = PathBuf::from(source);
            if path.is_file() {
                let yaml = std::fs::read_to_string(&path)?;
                let app = schema::parse(&yaml).map_err(err)?;
                std::fs::write(dest.join(format!("{}.yaml", app.id)), yaml)?;
                ids.push(app.id);
            } else if path.is_dir() {
                copy_defs(&path, &dest)?;
            } else {
                return Err(err(format!("{source} is not a file, a folder or a Git URL")));
            }
        }

        if ids.is_empty() {
            let _ = std::fs::remove_dir_all(&dest);
            return Err(err("no valid Quick App definitions were found there"));
        }
        self.meta.origins.insert(folder_name, source.to_string());
        self.persist()?;
        Ok(ids)
    }

    /// Removes every recipe an enabled plugin put here (Stage 16); they are re-added from the
    /// plugins that are still on.
    pub fn clear_plugin_sources(&mut self) {
        if let Ok(dirs) = std::fs::read_dir(self.imported_dir()) {
            for d in dirs.flatten() {
                if d.file_name().to_string_lossy().starts_with("plugin-") {
                    let _ = std::fs::remove_dir_all(d.path());
                }
            }
        }
        self.meta.origins.retain(|k, _| !k.starts_with("plugin-"));
        self.meta.trusted_sources.retain(|o| !o.starts_with("plugin:"));
    }

    /// A plugin's recipes: imported like any other source, and trusted because the user approved
    /// the plugin's `quick_apps` permission (each recipe still shows its review before it runs).
    pub fn import_plugin(&mut self, plugin_id: &str, dir: &Path) -> Result<Vec<String>, CoreError> {
        let folder = format!("plugin-{plugin_id}");
        let origin = format!("plugin:{plugin_id}");
        let dest = self.imported_dir().join(&folder);
        std::fs::create_dir_all(&dest)?;
        let mut ids = Vec::new();
        for (app, yaml, _) in Self::read_dir_defs(dir) {
            std::fs::write(dest.join(format!("{}.yaml", app.id)), yaml)?;
            ids.push(app.id);
        }
        if ids.is_empty() {
            let _ = std::fs::remove_dir_all(&dest);
            return Ok(ids);
        }
        self.meta.origins.insert(folder, origin.clone());
        self.meta.trusted_sources.insert(origin);
        self.persist()?;
        Ok(ids)
    }

    /// §88 "Trust Source": approves every recipe that came from `origin`.
    pub fn trust_source(&mut self, origin: &str) -> Result<(), CoreError> {
        self.meta.trusted_sources.insert(origin.to_string());
        self.persist()
    }

    pub fn untrust_source(&mut self, origin: &str) -> Result<(), CoreError> {
        self.meta.trusted_sources.remove(origin);
        self.persist()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> (QuickCatalog, crate::test_support::IsolatedHome) {
        let home = crate::test_support::isolated_home();
        (QuickCatalog::load(&home.paths).unwrap(), home)
    }

    #[test]
    fn all_builtin_apps_parse_and_are_trusted() {
        let (cat, _h) = catalog();
        let list = cat.list();
        let ids: Vec<&str> = list.iter().map(|e| e.id.as_str()).collect();
        for want in [
            "laravel", "symfony", "wordpress", "react-vite", "vue-vite", "nextjs", "express-api", "fastapi", "django", "plain-php",
            "static-html", "custom-app", "reverse-proxy",
        ] {
            assert!(ids.contains(&want), "missing built-in {want}: {ids:?}");
        }
        assert_eq!(list.len(), 13);
        assert!(list.iter().all(|e| e.trusted && e.source == EntrySource::Builtin));
    }

    /// Every shipped recipe must render with its own defaults — a template typo in a YAML
    /// file otherwise only shows up when a user clicks "Create".
    #[test]
    fn every_builtin_plans_cleanly_with_default_answers() {
        use super::super::plan::{build_plan, resolve_values, PlanCtx, StepBody};
        let (cat, _h) = catalog();
        let ctx = PlanCtx { projects_dir: "C:\\Sites".into(), web_server: "nginx".into(), http_port: 80, https_port: 443 };
        for view in cat.list() {
            let app = cat.get(&view.id).unwrap().app;
            let provided = BTreeMap::from([("project_name".to_string(), "demo-app".to_string())]);
            let values = resolve_values(&app, &provided, &ctx).unwrap_or_else(|e| panic!("{}: {e:?}", app.id));
            let plan = build_plan(&app, values, &ctx).unwrap_or_else(|e| panic!("{}: {e}", app.id));
            assert!(!plan.steps.is_empty(), "{} produced an empty plan", app.id);
            if app.domain.is_some() {
                assert!(plan.hostname.is_some(), "{} should create a domain", app.id);
                assert!(plan.steps.iter().any(|s| matches!(&s.body, StepBody::Action { action, .. } if action == "apply_web")));
            }
            // Nothing may write outside the project folder.
            for s in &plan.steps {
                if let StepBody::WriteFile { path, .. } = &s.body {
                    assert!(path.starts_with("C:\\Sites\\demo-app"), "{}: {path}", app.id);
                }
            }
        }
    }

    #[test]
    fn laravel_plan_matches_the_srs_result_list() {
        use super::super::plan::{build_plan, resolve_values, PlanCtx, StepBody};
        let (cat, _h) = catalog();
        let ctx = PlanCtx { projects_dir: "C:\\Sites".into(), web_server: "nginx".into(), http_port: 80, https_port: 443 };
        let app = cat.get("laravel").unwrap().app;
        let provided = BTreeMap::from([("project_name".to_string(), "shop".to_string())]);
        let plan = build_plan(&app, resolve_values(&app, &provided, &ctx).unwrap(), &ctx).unwrap();

        let ids: Vec<&str> = plan.requirements.iter().map(|r| r.id.as_str()).collect();
        for want in ["php", "composer", "node", "mariadb", "mailpit", "nginx"] {
            assert!(ids.contains(&want), "requirement {want} missing: {ids:?}");
        }
        assert_eq!(plan.hostname.as_deref(), Some("shop.test"));
        let names: Vec<&str> = plan.steps.iter().map(|s| s.name.as_str()).collect();
        for want in ["Create the database", "Create the Laravel project", "Run migrations", "Create shop.test", "Trust the local certificate authority", "Apply web server config", "Health checks"] {
            assert!(names.contains(&want), "step {want} missing: {names:?}");
        }
        let create = plan.steps.iter().find(|s| s.name == "Create the Laravel project").unwrap();
        let StepBody::Run { program, args, cwd } = &create.body else { panic!() };
        assert_eq!(program, "composer");
        assert_eq!(args, &["create-project", "laravel/laravel", "C:\\Sites\\shop", "--no-interaction", "--prefer-dist"]);
        assert_eq!(cwd.as_deref(), Some("C:\\Sites"));
        // The database is created before migrations run against it.
        let pos = |n: &str| names.iter().position(|x| *x == n).unwrap();
        assert!(pos("Create the database") < pos("Run migrations"));
    }

    #[test]
    fn local_edit_overrides_a_builtin_and_deleting_restores_it() {
        let (mut cat, _h) = catalog();
        let mut yaml = cat.get("plain-php").unwrap().yaml;
        yaml = yaml.replace("name: Plain PHP", "name: My PHP");
        cat.save(&yaml).unwrap();

        let detail = cat.get("plain-php").unwrap();
        assert_eq!(detail.app.name, "My PHP");
        assert_eq!(detail.view.source, EntrySource::Local);
        assert!(detail.view.overrides_builtin);

        cat.delete("plain-php").unwrap();
        assert_eq!(cat.get("plain-php").unwrap().app.name, "Plain PHP");
        assert!(cat.delete("plain-php").is_err(), "built-ins can't be deleted");
    }

    #[test]
    fn duplicate_creates_an_independent_local_copy() {
        let (mut cat, _h) = catalog();
        let copy = cat.duplicate("static-html", "my-site", "My Site").unwrap();
        assert_eq!(copy.view.source, EntrySource::Local);
        assert_eq!(copy.app.name, "My Site");
        assert!(cat.duplicate("static-html", "my-site", "again").is_err(), "id already taken");
        assert!(cat.duplicate("static-html", "Bad Id", "x").is_err());
    }

    #[test]
    fn imports_are_untrusted_until_their_source_is_trusted() {
        let (mut cat, home) = catalog();
        let dir = home.paths.root().join("catalog-src");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("evil.yaml");
        std::fs::write(&file, "id: evil-app\nname: Evil\ncommands:\n  - echo hi\n").unwrap();

        let ids = cat.import(file.to_str().unwrap()).unwrap();
        assert_eq!(ids, ["evil-app"]);
        let view = cat.get("evil-app").unwrap().view;
        assert_eq!(view.source, EntrySource::Imported);
        assert!(!view.trusted, "imports must start untrusted (§139)");

        cat.trust_source(view.origin.as_deref().unwrap()).unwrap();
        assert!(cat.get("evil-app").unwrap().view.trusted);
        cat.untrust_source(view.origin.as_deref().unwrap()).unwrap();
        assert!(!cat.get("evil-app").unwrap().view.trusted);
    }

    #[test]
    fn an_import_cannot_replace_a_builtin_and_editing_an_import_is_refused() {
        let (mut cat, home) = catalog();
        let file = home.paths.root().join("laravel.yaml");
        std::fs::write(&file, "id: laravel\nname: Not Laravel\ncommands:\n  - calc.exe\n").unwrap();
        cat.import(file.to_str().unwrap()).unwrap();
        assert_eq!(cat.get("laravel").unwrap().app.name, "Laravel", "the built-in must win over an import");

        let other = home.paths.root().join("x.yaml");
        std::fs::write(&other, "id: theirs\nname: Theirs\ncommands:\n  - echo\n").unwrap();
        cat.import(other.to_str().unwrap()).unwrap();
        assert!(cat.save("id: theirs\nname: Mine\ncommands:\n  - echo\n").is_err());
    }

    #[test]
    fn invalid_imports_are_rejected_and_leave_nothing_behind() {
        let (mut cat, home) = catalog();
        let file = home.paths.root().join("bad.yaml");
        std::fs::write(&file, "id: Bad Id\nname: x").unwrap();
        assert!(cat.import(file.to_str().unwrap()).is_err());
        assert!(cat.import("C:/definitely/not/here").is_err());
        assert!(cat.list().iter().all(|e| e.source == EntrySource::Builtin));
    }

    #[test]
    fn favorites_and_export_work() {
        let (mut cat, home) = catalog();
        cat.set_favorite("laravel", true).unwrap();
        assert!(cat.list().iter().find(|e| e.id == "laravel").unwrap().favorite);
        // Persisted.
        assert!(QuickCatalog::load(&home.paths).unwrap().list().iter().find(|e| e.id == "laravel").unwrap().favorite);

        let dest = home.paths.root().join("out.yaml");
        cat.export("laravel", dest.to_str().unwrap()).unwrap();
        assert_eq!(super::schema::parse(&std::fs::read_to_string(dest).unwrap()).unwrap().id, "laravel");
    }
}
