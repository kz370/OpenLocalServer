import type { AiProvider, AiRequest } from './types/ai'
import type { QuickCommand, StartupSettings } from './types/apps'
import type { CustomServiceDef, EnvironmentManifest, Ownership, ProcessId, ProcessSpec } from './types/base'
import type { XdebugSettings } from './types/environment'
import type { LoadProfile } from './types/system'
import type { AutoBackupSettings, AutoBackupSiteSettings, BundleOptions, OnConflict, Profile, ResourceLimits, RestoreOptions, ScheduledTask, SnapshotOptions, TunnelConfig, Worker } from './types/tasks'
import type { ConfigPart, Domain, ExternalTool } from './types/web'

export type CoreCommand =
  | { type: 'ping' }
  | { type: 'get_setting'; key: string }
  | { type: 'set_setting'; key: string; value: unknown }
  | { type: 'get_secret'; key: string }
  | { type: 'set_secret'; key: string; value: string }
  | { type: 'start_process'; spec: ProcessSpec }
  | { type: 'stop_process'; id: ProcessId }
  | { type: 'list_processes' }
  | { type: 'get_process_output'; id: ProcessId }
  | { type: 'run_command'; executable: string; args: string[]; cwd: string | null; timeout_ms: number }
  | { type: 'list_command_history' }
  | { type: 'check_port'; port: number }
  | { type: 'list_runtime_catalog' }
  | { type: 'refresh_runtime_catalog'; id: string }
  | { type: 'install_runtime'; id: string; version: string }
  | { type: 'pause_runtime'; id: string; version: string; paused: boolean }
  | { type: 'cancel_runtime'; id: string; version: string }
  | { type: 'remove_runtime'; id: string; version: string }
  | { type: 'register_project'; path: string }
  | { type: 'scan_and_register_projects'; path: string }
  | { type: 'list_projects' }
  | { type: 'sync_auto_domains' }
  | { type: 'remove_project'; id: string }
  | { type: 'delete_project'; id: string }
  | { type: 'rename_ignored_folder'; old_name: string; new_name: string }
  | { type: 'list_deleted_items' }
  | { type: 'forget_deleted_item'; value: string }
  | { type: 'clear_deleted_items' }
  | { type: 'get_project_detail'; id: string }
  | { type: 'inspect_site_folder'; path: string }
  | { type: 'run_in_project'; project_id: string; runtime_id: string; args: string[] }
  | { type: 'list_wp_projects' }
  | { type: 'wp_sign_in'; project_id: string; hostname?: string; theme?: 'light' | 'dark' }
  | { type: 'wp_sign_in_revoke'; project_id: string }
  | { type: 'list_services' }
  | { type: 'start_service'; id: string }
  | { type: 'stop_service'; id: string }
  /**
   * Stop everything: tunnels, supervised workers, the web stack (servers + PHP pools)
   * and every running service, then a supervisor sweep. Answers with whatever survived,
   * which is empty in the only case where the status mark can reach red.
   */
  | { type: 'stop_all' }
  | { type: 'list_db_tools' }
  | { type: 'open_db_tool'; id: string }
  | { type: 'set_custom_install'; id: string; label: string; path: string }
  | { type: 'remove_custom_install'; id: string; label: string }
  | { type: 'list_custom_installs' }
  | { type: 'scan_php_folder'; dir: string }
  | { type: 'list_php_extensions'; version: string }
  | { type: 'set_php_extension'; version: string; name: string; enabled: boolean }
  | { type: 'install_php_extension'; version: string; name: string }
  | { type: 'list_pecl_packages' }
  | { type: 'get_web_status' }
  | { type: 'get_web_config' }
  | { type: 'list_domains' }
  | { type: 'get_domain'; hostname: string }
  | { type: 'add_domain'; domain: Domain }
  | { type: 'update_domain'; domain: Domain }
  | { type: 'remove_domain'; hostname: string }
  | { type: 'delete_site'; hostname: string }
  | { type: 'set_domain_enabled'; hostname: string; enabled: boolean }
  | { type: 'bulk_set_domains_enabled'; hostnames: string[]; enabled: boolean }
  | { type: 'bulk_remove_domains'; hostnames: string[] }
  | { type: 'bulk_delete_sites'; hostnames: string[] }
  | { type: 'bulk_set_domain_server'; hostnames: string[]; server: string | null }
  | { type: 'duplicate_domain'; hostname: string; new_hostname: string }
  | { type: 'rename_domain'; hostname: string; new_hostname: string }
  | { type: 'set_domain_icon'; hostname: string; icon?: string | null }
  | { type: 'set_site_icon_image'; hostname?: string | null; folder?: string | null; webp_base64: string }
  | { type: 'remove_site_icon_image'; hostname?: string | null; folder?: string | null }
  | { type: 'read_image_file'; path: string }
  | { type: 'suggest_domain'; project_id: string; template: string }
  | { type: 'apply_web'; overwrite: string[] }
  | { type: 'stop_web' }
  | { type: 'validate_web' }
  | { type: 'restart_site_app'; hostname: string }
  | { type: 'sync_hosts' }
  | { type: 'get_ca_info' }
  | { type: 'trust_ca' }
  | { type: 'untrust_ca' }
  | { type: 'list_certificates' }
  | { type: 'regenerate_certificate'; hostname: string }
  | { type: 'revoke_certificate'; hostname: string }
  | { type: 'verify_ssl_files'; cert_path: string; key_path: string }
  | { type: 'issue_lets_encrypt'; hostname: string; email?: string | null }
  | { type: 'sync_ddns'; hostname: string; force: boolean }
  | { type: 'detect_public_ip' }
  | { type: 'sync_all_ddns'; force: boolean }
  | { type: 'health_check'; hostname: string }
  | { type: 'list_web_configs' }
  | { type: 'read_web_config'; hostname: string | null; part: ConfigPart; server?: string }
  | { type: 'write_web_config'; hostname: string; part: ConfigPart; content: string }
  | { type: 'set_ownership'; hostname: string; ownership: Ownership }
  | { type: 'list_config_history'; hostname: string }
  | { type: 'read_config_history'; hostname: string; id: string }
  | { type: 'restore_config_history'; hostname: string; id: string }
  | { type: 'export_web_config'; hostname: string; part: ConfigPart; dest: string }
  | { type: 'create_database'; engine: string; name: string; user?: string | null; password?: string | null }
  | { type: 'list_databases'; engine: string }
  | { type: 'list_custom_services' }
  | { type: 'save_custom_service'; service: CustomServiceDef }
  | { type: 'remove_custom_service'; id: string }
  | { type: 'backup_database'; engine: string; database: string }
  | { type: 'list_db_backups'; engine: string; database: string | null }
  | { type: 'restore_database'; engine: string; database: string; file: string }
  | { type: 'delete_db_backup'; engine: string; file: string }
  | { type: 'create_db_user'; engine: string; user: string; password: string; database: string }
  | { type: 'list_db_users'; engine: string }
  | { type: 'get_connection_info'; engine: string; database: string | null; path: string | null }
  | { type: 'list_sqlite' }
  | { type: 'detect_sqlite'; project_id: string }
  | { type: 'create_sqlite'; path: string; project_id: string | null }
  | { type: 'associate_sqlite'; path: string; project_id: string | null }
  | { type: 'forget_sqlite'; path: string }
  | { type: 'backup_sqlite'; path: string }
  | { type: 'restore_sqlite'; path: string; backup: string }
  | { type: 'check_sqlite'; path: string }
  | { type: 'list_external_tools' }
  | { type: 'save_external_tool'; tool: ExternalTool }
  | { type: 'remove_external_tool'; id: string }
  | { type: 'open_database'; engine: string; database: string | null; path: string | null; tool_id: string | null }
  | { type: 'list_quick_apps' }
  | { type: 'get_quick_app'; id: string }
  | { type: 'save_quick_app'; yaml: string }
  | { type: 'duplicate_quick_app'; id: string; new_id: string; new_name: string }
  | { type: 'delete_quick_app'; id: string }
  | { type: 'favorite_quick_app'; id: string; favorite: boolean }
  | { type: 'export_quick_app'; id: string; dest: string }
  | { type: 'import_quick_app'; source: string }
  | { type: 'trust_quick_app_source'; origin: string }
  | { type: 'plan_quick_app'; id: string; values: Record<string, string> }
  | {
    type: 'start_quick_app'
    id: string
    values: Record<string, string>
    approval: 'once' | 'source' | null
    allow_elevated: boolean
  }
  | { type: 'get_quick_run'; id: string }
  | { type: 'list_quick_runs' }
  | { type: 'cancel_quick_run'; id: string }
  | { type: 'list_quick_commands' }
  | { type: 'save_quick_command'; command: QuickCommand }
  | { type: 'delete_quick_command'; id: string }
  | { type: 'run_quick_command'; id: string; project_id: string | null }
  | { type: 'run_command_line'; line: string; cwd: string | null; project_id: string | null }
  | { type: 'list_history' }
  | { type: 'delete_history'; id: number }
  | { type: 'clear_history' }
  | { type: 'save_history_as_quick_command'; id: number; command_id: string; name: string }
  | { type: 'get_dashboard' }
  | { type: 'get_environment_health' }
  | { type: 'list_log_sources' }
  | { type: 'read_log'; source: string; max_lines: number }
  | { type: 'export_log'; source: string; dest: string }
  | { type: 'clear_log'; source: string }
  | { type: 'get_startup_settings' }
  | { type: 'set_startup_settings'; settings: StartupSettings }
  | { type: 'open_path'; path: string }
  | { type: 'open_url'; url: string }
  | { type: 'open_in_editor'; path: string }
  | { type: 'open_with'; path: string; app: string }
  | { type: 'open_terminal'; project_id: string | null; shell: string | null; rows: number; cols: number }
  | { type: 'terminal_input'; id: number; data: string }
  | { type: 'resize_terminal'; id: number; rows: number; cols: number }
  | { type: 'close_terminal'; id: number }
  | { type: 'list_project_shortcuts'; project_id: string }
  | { type: 'list_editors' }
  | { type: 'get_system_stats' }
  | { type: 'list_migration_sources' }
  | { type: 'list_foreign_databases'; source_id: string; password: string }
  | { type: 'migrate_databases'; source_id: string; password: string; databases: string[]; target: string }
  | { type: 'get_migration_progress' }
  | { type: 'get_helper_service' }
  | { type: 'install_helper_service' }
  | { type: 'uninstall_helper_service' }
  | { type: 'get_xdebug'; version: string }
  | { type: 'set_xdebug'; version: string; settings: XdebugSettings }
  | { type: 'xdebug_ide_config'; project_id: string; ide: string; version: string }
  | { type: 'discover_commands'; project_id: string }
  | { type: 'get_composer_info'; project_id: string }
  | { type: 'run_composer'; project_id: string; action: string; target: string | null }
  | { type: 'get_package_managers'; project_id: string }
  | { type: 'enable_package_manager'; project_id: string; manager: string }
  | { type: 'get_venv'; project_id: string }
  | { type: 'create_venv'; project_id: string; recreate: boolean }
  | { type: 'install_venv_requirements'; project_id: string; what: string }
  | { type: 'run_diagnostics' }
  | { type: 'ignore_diagnostic'; id: string; ignore: boolean }
  | { type: 'restart_service'; id: string }
  | { type: 'list_env_files'; project_id: string }
  | { type: 'read_env_file'; project_id: string; file: string }
  | { type: 'save_env_file'; project_id: string; file: string; content: string }
  | { type: 'set_env_value'; project_id: string; file: string; key: string; value: string }
  | { type: 'delete_env_key'; project_id: string; file: string; key: string }
  | { type: 'compare_env_files'; project_id: string; a: string; b: string }
  | { type: 'import_env_file'; project_id: string; file: string; source: string; mode: 'merge' | 'replace' }
  | { type: 'export_env_file'; project_id: string; file: string; dest: string }
  | { type: 'create_env_file'; project_id: string; file: string; from: string | null }
  | { type: 'list_operations' }
  | { type: 'dismiss_operation'; id: number }
  | { type: 'mailpit_env_plan'; project_id: string; file: string }
  | { type: 'apply_mailpit_env'; project_id: string; file: string }
  | { type: 'mail_diagnostics'; project_id: string | null }
  | { type: 'send_test_mail'; to: string }
  // Stage 12
  | { type: 'get_manifest'; project_id: string }
  | { type: 'save_manifest'; project_id: string; manifest: EnvironmentManifest | null }
  | { type: 'save_manifest_text'; project_id: string; text: string }
  | { type: 'plan_setup'; project_id: string }
  | { type: 'apply_setup'; project_id: string; dry_run: boolean }
  | { type: 'get_setup_progress' }
  | { type: 'import_procfile'; project_id: string; dry_run: boolean }
  | { type: 'read_site_file'; hostname: string; name: '.htaccess' }
  | { type: 'write_site_file'; hostname: string; name: '.htaccess'; content: string }
  // Stage 13
  | { type: 'list_profiles' }
  | { type: 'save_profile'; profile: Profile }
  | { type: 'delete_profile'; id: string }
  | { type: 'export_profile'; id: string; dest: string }
  | { type: 'read_profile_file'; source: string }
  | { type: 'import_profile'; source: string }
  | { type: 'apply_profile'; project_id: string; profile_id: string }
  | { type: 'profile_from_project'; project_id: string; name: string }
  | { type: 'profile_yaml'; id: string }
  | { type: 'save_profile_yaml'; yaml: string }
  | { type: 'get_project_modes'; project_id: string }
  | { type: 'set_project_mode'; project_id: string; mode: string }
  | { type: 'list_workers'; project_id: string | null }
  | { type: 'list_worker_presets' }
  | { type: 'save_worker'; worker: Worker }
  | { type: 'remove_worker'; id: string }
  | { type: 'start_worker'; id: string }
  | { type: 'stop_worker'; id: string }
  | { type: 'restart_worker'; id: string }
  | { type: 'start_project_workers'; project_id: string }
  | { type: 'stop_project_workers'; project_id: string }
  | { type: 'list_schedules'; project_id: string | null }
  | { type: 'save_schedule'; task: ScheduledTask }
  | { type: 'remove_schedule'; id: string }
  | { type: 'run_schedule_now'; id: string }
  | { type: 'describe_schedule'; schedule: string }
  | { type: 'list_snapshots'; project_id: string }
  | { type: 'create_snapshot'; project_id: string; label: string; options: SnapshotOptions }
  | { type: 'delete_snapshot'; project_id: string; id: string }
  | { type: 'restore_snapshot'; project_id: string; id: string; options: RestoreOptions }
  | { type: 'export_snapshot'; project_id: string; id: string; dest: string }
  | { type: 'preview_import'; source: string }
  | { type: 'import_environment'; source: string; target: string; name: string }
  | { type: 'clone_environment'; project_id: string; target: string; name: string; what: 'full' | 'infrastructure' | 'configuration' }
  | { type: 'backup_settings' }
  | { type: 'list_settings_backups' }
  | { type: 'restore_settings'; id: string }
  | { type: 'get_resource_limits' }
  | { type: 'set_resource_limits'; limits: ResourceLimits }
  | { type: 'get_auto_backup' }
  | { type: 'set_auto_backup'; settings: AutoBackupSettings }
  | { type: 'set_site_auto_backup'; hostname: string; site: AutoBackupSiteSettings }
  | { type: 'run_auto_backup' }
  // Stage 14
  | { type: 'list_tunnel_providers' }
  | { type: 'list_tunnels' }
  | { type: 'save_tunnel'; tunnel: TunnelConfig }
  | { type: 'remove_tunnel'; id: string }
  | { type: 'start_tunnel'; id: string; confirm_exposure: boolean }
  | { type: 'stop_tunnel'; id: string }
  | { type: 'check_tunnel'; id: string }
  | { type: 'tunnel_log'; id: string }
  | { type: 'set_tunnel_token'; provider: string; token: string | null }
  | { type: 'set_tunnel_password'; id: string; password: string | null }
  | { type: 'list_tunnel_requests'; id: string }
  | { type: 'clear_tunnel_requests'; id: string }
  | { type: 'replay_tunnel_request'; id: string; request_id: number }
  | { type: 'send_tunnel_test_request'; id: string; method: string; path: string; headers: [string, string][]; body: string }
  // Stage 15
  | { type: 'global_search'; query: string }
  | { type: 'doctor' }
  | { type: 'diagnose_project'; project_id: string }
  | { type: 'plan_repair'; project_id: string | null }
  | { type: 'apply_repair'; project_id: string | null; ids: string[]; confirm_destructive: boolean }
  | { type: 'git_status'; project_id: string }
  | { type: 'git_init'; project_id: string }
  | { type: 'git_branches'; project_id: string }
  | { type: 'git_create_branch'; project_id: string; name: string; checkout: boolean }
  | { type: 'git_switch_branch'; project_id: string; name: string }
  | { type: 'git_delete_branch'; project_id: string; name: string; force: boolean }
  | { type: 'git_stage'; project_id: string; paths: string[] }
  | { type: 'git_unstage'; project_id: string; paths: string[] }
  | { type: 'git_discard'; project_id: string; paths: string[] }
  | { type: 'git_commit'; project_id: string; message: string; amend: boolean }
  | { type: 'git_sync'; project_id: string; action: 'pull' | 'push' | 'fetch'; remote: string | null }
  | { type: 'git_diff'; project_id: string; path: string; staged: boolean }
  | { type: 'git_log'; project_id: string; limit: number }
  | { type: 'git_show'; project_id: string; hash: string }
  | { type: 'git_add_remote'; project_id: string; name: string; url: string }
  | { type: 'git_remove_remote'; project_id: string; name: string }
  | { type: 'git_stash'; project_id: string; action: 'push' | 'pop' | 'apply' | 'drop'; message: string | null; index: number | null }
  | { type: 'git_add_ignore'; project_id: string; template: string }
  | { type: 'git_set_credentials'; host: string; username: string; token: string | null }
  | { type: 'list_ssh_keys' }
  | { type: 'git_clone'; url: string; target: string; branch: string | null; auth: { type: 'https'; username: string; password: string; remember: boolean } | { type: 'ssh'; key_path: string; remember: boolean; passphrase: string | null } | null }
  | { type: 'list_plugins' }
  | { type: 'install_plugin'; source: string }
  | { type: 'set_plugin_enabled'; id: string; enabled: boolean; approve: string[] }
  | { type: 'remove_plugin'; id: string }
  | { type: 'plugin_detect'; project_id: string }
  | { type: 'list_catalog_sources' }
  | { type: 'add_catalog_source'; name: string; url: string; public_key: string }
  | { type: 'remove_catalog_source'; id: string }
  | { type: 'refresh_catalogs'; id: string | null }
  | { type: 'install_catalog_plugin'; source_id: string; plugin_id: string }
  | { type: 'get_api_status' }
  | { type: 'set_api_settings'; enabled: boolean; port: number; mode: 'read_only' | 'operate' }
  | { type: 'rotate_api_token' }
  | { type: 'clear_api_token' }
  | { type: 'get_updater_status' }
  | { type: 'set_updater_settings'; endpoint: string; public_key: string }
  | { type: 'check_update' }
  | { type: 'download_update' }
  | { type: 'install_update' }
  | { type: 'get_shell_menu' }
  | { type: 'install_shell_menu' }
  | { type: 'remove_shell_menu' }
  | { type: 'check_network'; force: boolean }
  | { type: 'export_support_bundle'; dest: string }
  | { type: 'load_overview'; project_id: string }
  | { type: 'load_read_script'; project_id: string; name: string }
  | { type: 'load_save_script'; project_id: string; name: string; content: string }
  | { type: 'load_delete_script'; project_id: string; name: string }
  | { type: 'load_list_profiles' }
  | { type: 'load_save_profile'; profile: LoadProfile }
  | { type: 'load_delete_profile'; id: string }
  | { type: 'load_generate'; project_id: string; profile: LoadProfile; name: string | null }
  | { type: 'load_run'; project_id: string; script: string; target: string | null; confirm_public: boolean; env: [string, string][] }
  | { type: 'load_status'; run_id: string }
  | { type: 'load_stop'; run_id: string }
  | { type: 'load_runs'; project_id: string }
  | { type: 'load_delete_run'; project_id: string; run_id: string }
  | { type: 'ai_get_state' }
  | { type: 'ai_save_settings'; enabled: boolean; features: Record<string, string> }
  | { type: 'ai_save_provider'; provider: AiProvider; api_key: string | null }
  | { type: 'ai_remove_provider'; id: string }
  | { type: 'ai_detect_local' }
  | { type: 'ai_test'; provider_id: string }
  | { type: 'ai_models'; provider_id: string }
  | { type: 'ai_probe'; provider: AiProvider; api_key: string | null }
  | { type: 'ai_preview'; request: AiRequest }
  | { type: 'ai_start'; request: AiRequest; confirm_remote: boolean }
  | { type: 'ai_job'; job_id: string }
  | { type: 'ai_cancel'; job_id: string }
  | { type: 'ai_apply'; actions: CoreCommand[]; confirm_destructive: boolean }
  | { type: 'export_sites'; hostnames: string[]; options: BundleOptions; dest: string; password: string | null }
  | { type: 'preview_site_bundle'; source: string }
  | { type: 'import_sites'; source: string; password: string | null; on_conflict: OnConflict; name: string | null; options: RestoreOptions }
  | { type: 'list_tasks' }
  | { type: 'cancel_task'; id: string }
  | { type: 'dismiss_task'; id: string }
  | { type: 'clear_finished_tasks' }
  | { type: 'get_app_guide'; version?: string | null }
// @@ts-commands-end

