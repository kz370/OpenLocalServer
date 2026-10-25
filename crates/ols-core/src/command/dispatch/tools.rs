use super::super::types::*;
use super::super::Core;
use crate::error::CoreError;

impl Core {
    pub(crate) fn dispatch_tools(
        &self,
        command: CoreCommand,
    ) -> Result<Result<CoreResponse, CoreError>, CoreCommand> {
        use CoreCommand as C;
        use CoreResponse as R;
        let i = &self.inner;
        #[allow(unreachable_patterns)]
        let mut unhandled = None;
        let outcome: Result<CoreResponse, CoreError> = (|| {
            let res = match command {
                C::GitInit { project_id } => Ok(R::GitStatus {
                    status: Box::new(i.git_init(&project_id)?),
                }),
                C::GitBranches { project_id } => Ok(R::GitBranches {
                    branches: i.git_branches(&project_id)?,
                }),
                C::GitCreateBranch {
                    project_id,
                    name,
                    checkout,
                } => {
                    i.git_create_branch(&project_id, &name, checkout)?;
                    Ok(R::GitBranches {
                        branches: i.git_branches(&project_id)?,
                    })
                }
                C::GitSwitchBranch { project_id, name } => {
                    i.git_switch(&project_id, &name)?;
                    Ok(R::GitStatus {
                        status: Box::new(i.git_status(&project_id)?),
                    })
                }
                C::GitDeleteBranch {
                    project_id,
                    name,
                    force,
                } => {
                    i.git_delete_branch(&project_id, &name, force)?;
                    Ok(R::GitBranches {
                        branches: i.git_branches(&project_id)?,
                    })
                }
                C::GitStage { project_id, paths } => {
                    i.git_stage(&project_id, &paths)?;
                    Ok(R::GitStatus {
                        status: Box::new(i.git_status(&project_id)?),
                    })
                }
                C::GitUnstage { project_id, paths } => {
                    i.git_unstage(&project_id, &paths)?;
                    Ok(R::GitStatus {
                        status: Box::new(i.git_status(&project_id)?),
                    })
                }
                C::GitDiscard { project_id, paths } => {
                    tracing::info!(command = "git_discard", project = %project_id, files = paths.len());
                    i.git_discard(&project_id, &paths)?;
                    Ok(R::GitStatus {
                        status: Box::new(i.git_status(&project_id)?),
                    })
                }
                C::GitCommit {
                    project_id,
                    message,
                    amend,
                } => Ok(R::GitCommit {
                    commit: i.git_commit(&project_id, &message, amend)?,
                }),
                C::GitSync {
                    project_id,
                    action,
                    remote,
                } => {
                    tracing::info!(command = "git_sync", project = %project_id, action = %action);
                    Ok(R::GitResult {
                        result: i.git_sync(&project_id, &action, remote.as_deref())?,
                    })
                }
                C::GitDiff {
                    project_id,
                    path,
                    staged,
                } => Ok(R::Text {
                    text: i.git_diff(&project_id, &path, staged)?,
                }),
                C::GitLog { project_id, limit } => Ok(R::GitCommits {
                    commits: i.git_log(&project_id, limit)?,
                }),
                C::GitShow { project_id, hash } => Ok(R::Text {
                    text: i.git_show(&project_id, &hash)?,
                }),
                C::GitAddRemote {
                    project_id,
                    name,
                    url,
                } => {
                    i.git_add_remote(&project_id, &name, &url)?;
                    Ok(R::GitStatus {
                        status: Box::new(i.git_status(&project_id)?),
                    })
                }
                C::GitRemoveRemote { project_id, name } => {
                    i.git_remove_remote(&project_id, &name)?;
                    Ok(R::GitStatus {
                        status: Box::new(i.git_status(&project_id)?),
                    })
                }
                C::GitStash {
                    project_id,
                    action,
                    message,
                    index,
                } => Ok(R::GitResult {
                    result: i.git_stash(&project_id, &action, message.as_deref(), index)?,
                }),
                C::GitAddIgnore {
                    project_id,
                    template,
                } => Ok(R::Count {
                    count: i.git_add_ignore(&project_id, &template)?,
                }),
                C::GitSetCredentials {
                    host,
                    username,
                    token,
                } => {
                    // The token is never logged (§141).
                    tracing::info!(command = "git_set_credentials", host = %host);
                    i.git_set_credentials(&host, &username, token.as_deref())?;
                    Ok(R::Ok)
                }
                C::ListSshKeys => Ok(R::SshKeys {
                    keys: i.list_ssh_keys(),
                }),
                C::GitClone {
                    url,
                    target,
                    branch,
                    auth,
                } => {
                    tracing::info!(command = "git_clone", target = %target);
                    Ok(R::Project {
                        project: i.git_clone(&url, &target, branch.as_deref(), auth)?,
                    })
                }

                C::ListPlugins => Ok(R::Plugins {
                    plugins: i.list_plugins(),
                }),
                C::InstallPlugin { source } => {
                    tracing::info!(command = "install_plugin", source = %source);
                    Ok(R::Plugin {
                        plugin: Box::new(i.install_plugin(&source)?),
                    })
                }
                C::SetPluginEnabled {
                    id,
                    enabled,
                    approve,
                } => {
                    tracing::info!(command = "set_plugin_enabled", id = %id, enabled);
                    Ok(R::Plugin {
                        plugin: Box::new(i.set_plugin_enabled(&id, enabled, &approve)?),
                    })
                }
                C::RemovePlugin { id } => {
                    i.remove_plugin(&id)?;
                    Ok(R::Ok)
                }
                C::PluginDetect { project_id } => Ok(R::PluginDetections {
                    detections: i.plugin_detect(&project_id)?,
                }),
                C::ListCatalogSources => Ok(R::CatalogSources {
                    catalogs: i.catalog_views(),
                }),
                C::AddCatalogSource {
                    name,
                    url,
                    public_key,
                } => {
                    i.add_catalog_source(&name, &url, &public_key)?;
                    Ok(R::CatalogSources {
                        catalogs: i.catalog_views(),
                    })
                }
                C::RemoveCatalogSource { id } => {
                    i.remove_catalog_source(&id)?;
                    Ok(R::CatalogSources {
                        catalogs: i.catalog_views(),
                    })
                }
                C::RefreshCatalogs { id } => Ok(R::CatalogSources {
                    catalogs: i.refresh_catalogs(id.as_deref()),
                }),
                C::InstallCatalogPlugin {
                    source_id,
                    plugin_id,
                } => {
                    tracing::info!(command = "install_catalog_plugin", source = %source_id, plugin = %plugin_id);
                    Ok(R::Plugin {
                        plugin: Box::new(i.install_catalog_plugin(&source_id, &plugin_id)?),
                    })
                }

                C::GetApiStatus => Ok(R::ApiStatus {
                    status: Box::new(i.api_status()),
                }),
                C::SetApiSettings {
                    enabled,
                    port,
                    mode,
                } => {
                    tracing::info!(command = "set_api_settings", enabled, port, mode = %mode);
                    Ok(R::ApiStatus {
                        status: Box::new(i.set_api_settings(enabled, port, &mode)?),
                    })
                }
                C::RotateApiToken => {
                    tracing::info!(command = "rotate_api_token");
                    Ok(R::Text {
                        text: i.rotate_api_token()?,
                    })
                }
                C::ClearApiToken => {
                    i.clear_api_token()?;
                    Ok(R::ApiStatus {
                        status: Box::new(i.api_status()),
                    })
                }
                C::GetUpdaterStatus => Ok(R::UpdaterStatus {
                    status: Box::new(i.updater_status()),
                }),
                C::SetUpdaterSettings {
                    endpoint,
                    public_key,
                } => Ok(R::UpdaterStatus {
                    status: Box::new(i.set_updater_settings(&endpoint, &public_key)?),
                }),
                C::CheckUpdate => Ok(R::Update {
                    update: Box::new(i.check_update()?),
                }),
                C::DownloadUpdate => Ok(R::Update {
                    update: Box::new(i.download_update()?),
                }),
                C::InstallUpdate => {
                    tracing::info!(command = "install_update");
                    i.install_update()?;
                    Ok(R::Ok)
                }
                C::GetShellMenu => Ok(R::ShellMenu {
                    status: i.shell_menu_status(),
                }),
                C::InstallShellMenu => Ok(R::ShellMenu {
                    status: i.install_shell_menu()?,
                }),
                C::RemoveShellMenu => Ok(R::ShellMenu {
                    status: i.remove_shell_menu()?,
                }),
                C::CheckNetwork { force } => Ok(R::Network {
                    status: Box::new(i.network_status(force)),
                }),
                C::ExportSupportBundle { dest } => Ok(R::Lines {
                    lines: i.export_support_bundle(&dest)?,
                }),

                C::LoadOverview { project_id } => Ok(R::LoadOverview {
                    overview: Box::new(i.load_overview(&project_id)?),
                }),
                C::LoadReadScript { project_id, name } => Ok(R::Text {
                    text: i.load_read_script(&project_id, &name)?,
                }),
                C::LoadSaveScript {
                    project_id,
                    name,
                    content,
                } => {
                    i.load_save_script(&project_id, &name, &content)?;
                    Ok(R::Ok)
                }
                C::LoadDeleteScript { project_id, name } => {
                    i.load_delete_script(&project_id, &name)?;
                    Ok(R::Ok)
                }
                C::LoadListProfiles => Ok(R::LoadProfiles {
                    profiles: i.load_profiles(),
                }),
                C::LoadSaveProfile { profile } => Ok(R::LoadProfiles {
                    profiles: i.load_save_profile(profile)?,
                }),
                C::LoadDeleteProfile { id } => Ok(R::LoadProfiles {
                    profiles: i.load_delete_profile(&id)?,
                }),
                C::LoadGenerate {
                    project_id,
                    profile,
                    name,
                } => Ok(R::Text {
                    text: i.load_generate(&project_id, &profile, name.as_deref())?,
                }),
                C::LoadRun {
                    project_id,
                    script,
                    target,
                    confirm_public,
                    env,
                } => {
                    tracing::info!(command = "load_run", project = %project_id, script = %script, public = confirm_public);
                    Ok(R::LoadRun {
                        run: Box::new(i.load_run(
                            &project_id,
                            &script,
                            target.as_deref(),
                            confirm_public,
                            &env,
                        )?),
                    })
                }
                C::LoadStatus { run_id } => Ok(R::LoadRun {
                    run: Box::new(i.load_status(&run_id)?),
                }),
                C::LoadStop { run_id } => Ok(R::LoadRun {
                    run: Box::new(i.load_stop(&run_id)?),
                }),
                C::LoadRuns { project_id } => Ok(R::LoadRuns {
                    runs: i.load_runs(&project_id),
                }),
                C::LoadDeleteRun { project_id, run_id } => {
                    i.load_delete_run(&project_id, &run_id)?;
                    Ok(R::Ok)
                }

                C::AiGetState => Ok(R::AiState {
                    state: Box::new(i.ai_state()),
                }),
                C::AiSaveSettings { enabled, features } => Ok(R::AiState {
                    state: Box::new(i.ai_save_settings(enabled, features)?),
                }),
                C::AiSaveProvider { provider, api_key } => {
                    // Only that a provider was saved, never its key (§141).
                    tracing::info!(command = "ai_save_provider", provider = %provider.name, key_changed = api_key.is_some());
                    Ok(R::AiState {
                        state: Box::new(i.ai_save_provider(provider, api_key)?),
                    })
                }
                C::AiRemoveProvider { id } => Ok(R::AiState {
                    state: Box::new(i.ai_remove_provider(&id)?),
                }),
                C::AiDetectLocal => Ok(R::AiDetected {
                    servers: i.ai_detect_local(),
                }),
                C::AiTest { provider_id } => Ok(R::AiTest {
                    result: i.ai_test(&provider_id)?,
                }),
                C::AiProbe { provider, api_key } => Ok(R::AiTest {
                    result: i.ai_probe(provider, api_key)?,
                }),
                C::AiModels { provider_id } => Ok(R::AiModels {
                    models: i.ai_models(&provider_id)?,
                }),
                C::AiPreview { request } => Ok(R::AiPrompt {
                    prompt: Box::new(i.ai_prompt(&request)?),
                }),
                C::AiStart {
                    request,
                    confirm_remote,
                } => {
                    tracing::info!(command = "ai_start", feature = %request.feature, confirm_remote);
                    Ok(R::AiJob {
                        job: Box::new(i.ai_start(request, confirm_remote)?),
                    })
                }
                C::AiJob { job_id } => Ok(R::AiJob {
                    job: Box::new(i.ai_job(&job_id)?),
                }),
                C::AiCancel { job_id } => Ok(R::AiJob {
                    job: Box::new(i.ai_cancel(&job_id)?),
                }),
                C::AiApply {
                    actions,
                    confirm_destructive,
                } => Ok(R::AiApplied {
                    steps: self.ai_apply(actions, confirm_destructive),
                }),
                C::GetAppGuide { version: _ } => Ok(R::AppGuide {
                    markdown: i.app_guide(),
                    version: "2.3.0".to_string(),
                }),
                // @@arms-end
                other => {
                    unhandled = Some(other);
                    return Ok(R::Ok);
                }
            };
            res
        })();
        if let Some(cmd) = unhandled {
            Err(cmd)
        } else {
            Ok(outcome)
        }
    }
}
