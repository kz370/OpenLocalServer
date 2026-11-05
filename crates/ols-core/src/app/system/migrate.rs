use crate::app::*;
use crate::error::CoreError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct MigrationRecovery {
    engine: String,
    database: String,
    dump: PathBuf,
    action: RecoveryAction,
    backup: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RecoveryAction {
    RestoreBackup,
    DropPartial,
}

impl Inner {
    /// Rolls back interrupted database imports before the app exposes the runtimes to users.
    /// Pending records are retained if recovery fails, so the next launch retries.
    pub(crate) fn recover_pending_migrations(&self) {
        let dir = self.paths.data_dir().join("database-migration-recovery");
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => {
                tracing::error!(%error, path = %dir.display(), "cannot inspect database migration recovery records");
                return;
            }
        };
        for entry in entries.flatten() {
            let pending = entry.path();
            if pending.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let applied = pending.with_extension("applied");
            let record: MigrationRecovery = match std::fs::read(&pending)
                .map_err(|error| error.to_string())
                .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|error| error.to_string()))
            {
                Ok(record) => record,
                Err(error) => {
                    tracing::error!(%error, path = %pending.display(), "cannot read database migration recovery record; it was retained");
                    continue;
                }
            };
            let title = format!("Recover interrupted import of {}", record.database);
            let result = if applied.exists() {
                Ok(())
            } else {
                self.journaled("database_recovery", &title, None, None, || {
                    self.recover_migration_record(&record).map_err(svc)
                })
                .map_err(|error| error.to_string())
            };
            match result {
                Ok(()) => {
                    let _ = std::fs::remove_file(&record.dump);
                    let _ = std::fs::remove_file(&pending);
                    let _ = std::fs::remove_file(applied);
                    tracing::info!(database = %record.database, "interrupted database import recovered");
                }
                Err(error) => {
                    tracing::error!(%error, database = %record.database, path = %pending.display(), "database import recovery failed; it will retry on next launch")
                }
            }
        }
        self.clean_interrupted_migration_dumps();
    }

    fn recover_migration_record(&self, record: &MigrationRecovery) -> Result<(), String> {
        if !self.services.is_running(&record.engine) {
            self.services.start(&record.engine)?;
        }
        let (_, port) = self.services.sql_client(&record.engine)?;
        let started = std::time::Instant::now();
        while std::net::TcpStream::connect_timeout(
            &([127, 0, 0, 1], port).into(),
            std::time::Duration::from_millis(300),
        )
        .is_err()
        {
            if started.elapsed() > std::time::Duration::from_secs(60) {
                return Err(format!("{} did not start", record.engine));
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        match &record.action {
            RecoveryAction::DropPartial => {
                self.services.run_sql(
                    &record.engine,
                    &format!("DROP DATABASE IF EXISTS {}", mysql_ident(&record.database)),
                )?;
            }
            RecoveryAction::RestoreBackup => {
                let backup = record
                    .backup
                    .as_ref()
                    .ok_or("recovery record has no safety backup")?;
                if !backup.is_file() {
                    return Err(format!("safety backup {} is missing", backup.display()));
                }
                self.services.run_sql(
                    &record.engine,
                    &format!("DROP DATABASE IF EXISTS {}", mysql_ident(&record.database)),
                )?;
                self.services.run_sql(
                    &record.engine,
                    &format!("CREATE DATABASE {}", mysql_ident(&record.database)),
                )?;
                crate::dbbackup::load_into(
                    &self.services,
                    &record.engine,
                    &record.database,
                    backup,
                )?;
            }
        }
        Ok(())
    }

    fn clean_interrupted_migration_dumps(&self) {
        let dir = self.paths.cache_dir();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            if name.starts_with("migrate-") && name.ends_with(".sql") {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

fn write_recovery_record(dir: &Path, record: &MigrationRecovery) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let id = format!(
        "{}-{nonce}-{}",
        std::process::id(),
        crate::domain::slugify(&record.database)
    );
    let path = dir.join(format!("{id}.json"));
    let temp = dir.join(format!("{id}.json.tmp"));
    let bytes = serde_json::to_vec(record).map_err(|error| error.to_string())?;
    use std::io::Write;
    let mut file = std::fs::File::create(&temp).map_err(|error| error.to_string())?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| error.to_string())?;
    std::fs::rename(&temp, &path).map_err(|error| error.to_string())?;
    Ok(path)
}

fn mark_recovery_applied(path: &Path) -> Result<(), String> {
    let marker = path.with_extension("applied");
    let temp = path.with_extension("applied.tmp");
    let file = std::fs::File::create(&temp).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    std::fs::rename(temp, marker).map_err(|error| error.to_string())
}

impl Inner {
    fn migration_source(&self, id: &str) -> Result<crate::migrate::MigrationSource, CoreError> {
        if let Some(source) = crate::migrate::detect_managed(&self.paths, &self.runtimes)
            .into_iter()
            .find(|s| s.id == id)
        {
            return Ok(source);
        }
        let source = crate::migrate::inspect_data_dir(std::path::Path::new(id)).map_err(svc)?;
        crate::migrate::reject_import_target(
            &self.paths,
            &self.runtimes,
            std::path::Path::new(&source.data_dir),
        )
        .map_err(svc)?;
        Ok(source)
    }

    /// Databases in an old Laragon/XAMPP/Wamp server (started on a copy if it isn't running).
    pub fn foreign_databases(
        &self,
        source_id: &str,
        password: &str,
    ) -> Result<Vec<String>, CoreError> {
        let source = self.migration_source(source_id)?;
        self.migration.begin("scan");
        let result = crate::migrate::Session::open(
            source,
            password,
            &self.paths.cache_dir(),
            &self.migration,
        )
        .and_then(|session| {
            self.migration.step("Reading the list of databases", None);
            session.databases()
        });
        self.migration.end();
        result.map_err(svc)
    }

    /// Copies databases from an old server (MySQL or MariaDB) into our MariaDB (`target`).
    /// An empty `databases` list means all of them. Each database reports on its own, so
    /// one failure doesn't stop the rest.
    pub fn migrate_databases(
        &self,
        source_id: &str,
        password: &str,
        databases: &[String],
        target: &str,
    ) -> Result<Vec<crate::migrate::MigratedDb>, CoreError> {
        self.migration.begin("import");
        // Legacy callers retain their command shape but now get the safe default behavior.
        let result = self.migrate_databases_inner(
            source_id,
            password,
            databases,
            target,
            crate::migrate::ConflictPolicy::Skip,
        );
        self.migration.end();
        result
    }

    pub fn migrate_databases_with_conflicts(
        &self,
        source_id: &str,
        password: &str,
        databases: &[String],
        target: &str,
        conflict_policy: crate::migrate::ConflictPolicy,
    ) -> Result<Vec<crate::migrate::MigratedDb>, CoreError> {
        self.migration.begin("import");
        let result =
            self.migrate_databases_inner(source_id, password, databases, target, conflict_policy);
        self.migration.end();
        result
    }

    fn migrate_databases_inner(
        &self,
        source_id: &str,
        password: &str,
        databases: &[String],
        target: &str,
        conflict_policy: crate::migrate::ConflictPolicy,
    ) -> Result<Vec<crate::migrate::MigratedDb>, CoreError> {
        let progress = &self.migration;
        let source = self.migration_source(source_id)?;
        let (_, port) = self.services.sql_client(target).map_err(svc)?;
        if source.running_port == Some(port) {
            return Err(svc(format!(
                "{} is running on port {port}, which our {target} needs. Stop it (in Laragon/XAMPP) and try again; its data is copied, not moved.",
                source.label
            )));
        }
        progress.step(format!("Starting our {target}"), None);
        if !self.services.is_running(target) {
            self.services.start(target).map_err(svc)?;
        }
        let started = std::time::Instant::now();
        while std::net::TcpStream::connect_timeout(
            &([127, 0, 0, 1], port).into(),
            std::time::Duration::from_millis(300),
        )
        .is_err()
        {
            if started.elapsed() > std::time::Duration::from_secs(60) {
                return Err(svc(format!("our {target} did not start")));
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }

        let work = self.paths.cache_dir();
        std::fs::create_dir_all(&work)?;
        let session =
            crate::migrate::Session::open(source, password, &work, progress).map_err(svc)?;
        let names = if databases.is_empty() {
            session.databases().map_err(svc)?
        } else {
            databases.to_vec()
        };
        progress.step("Measuring the databases", None);
        let sizes = session.sizes();
        let mut results = Vec::new();
        let mut target_names = self.services.list_databases(target).map_err(svc)?;
        let total = names.len();
        for (index, db) in names.into_iter().enumerate() {
            progress.database(index + 1, total, &db);
            let resolution = crate::migrate::resolve_conflict(&db, &target_names, conflict_policy);
            if resolution == crate::migrate::ConflictResolution::Skip {
                let result = crate::migrate::MigratedDb {
                    name: db,
                    ok: false,
                    detail:
                        "Skipped because a database with this name already exists in the target."
                            .into(),
                };
                tracing::info!(database = %result.name, ok = false, "database migration skipped");
                progress.finished(result.clone());
                results.push(result);
                continue;
            }
            let target_db = match &resolution {
                crate::migrate::ConflictResolution::Rename(name) => name.clone(),
                _ => db.clone(),
            };
            let target_exists = resolution == crate::migrate::ConflictResolution::Replace;

            let file = work.join(format!("migrate-{}.sql", crate::domain::slugify(&db)));
            progress.step(
                format!("Exporting {db}"),
                sizes.get(&db).copied().filter(|s| *s > 0),
            );
            let mut safety_backup = None;
            let mut recovery_record = None;
            let mut recovery_record_data = None;
            let outcome = session.dump(&db, &file, progress).and_then(|()| {
                crate::migrate::rename_dump_database(&file, &target_db)?;
                let recovery_dir = self.paths.data_dir().join("database-migration-recovery");
                if target_exists {
                    let backup = backup_before_replace(
                        || crate::dbbackup::backup(&self.services, &self.paths, target, &db)
                            .map_err(|e| format!("could not make a safety backup, so {db} was not changed ({e})")),
                        |backup| {
                            // Persist recovery intent after the durable backup, before touching the target.
                            let record = MigrationRecovery {
                                engine: target.to_owned(),
                                database: db.clone(),
                                dump: file.clone(),
                                action: RecoveryAction::RestoreBackup,
                                backup: Some(backup.to_path_buf()),
                            };
                            recovery_record = Some(write_recovery_record(&recovery_dir, &record)?);
                            recovery_record_data = Some(record);
                            safety_backup = Some(backup.to_path_buf());
                            self.services
                                .run_sql(target, &format!("DROP DATABASE {}", mysql_ident(&db)))
                                .map_err(|e| format!("could not remove the existing {db}; its safety backup is at {} ({e})", backup.display()))?;
                            self.services
                                .run_sql(target, &format!("CREATE DATABASE {}", mysql_ident(&db)))
                                .map_err(|e| format!("could not recreate {db} after its backup was secured; the app will restore it on next launch ({e})"))?;
                            Ok(())
                        },
                    )?;
                    safety_backup = Some(backup);
                } else {
                    // Strict CREATE guards against a database appearing after the initial scan.
                    // Only journal after CREATE succeeds, so recovery cannot drop that race winner.
                    self.services
                        .run_sql(target, &format!("CREATE DATABASE {}", mysql_ident(&target_db)))
                        .map_err(|e| format!("could not create destination database {target_db}: {e}"))?;
                    let record = MigrationRecovery {
                        engine: target.to_owned(),
                        database: target_db.clone(),
                        dump: file.clone(),
                        action: RecoveryAction::DropPartial,
                        backup: None,
                    };
                    recovery_record = Some(write_recovery_record(&recovery_dir, &record)?);
                    recovery_record_data = Some(record);
                }
                progress.step(
                    format!("Importing {db} as {target_db} into {target}"),
                    std::fs::metadata(&file).ok().map(|m| m.len()),
                );
                let (client, port) = self.services.sql_client(target).map_err(|e| e.to_string())?;
                crate::migrate::import(&client, port, &file, progress)
            });

            let detail = match outcome {
                Ok(()) => {
                    target_names.push(target_db.clone());
                    if let Some(record) = recovery_record.as_ref() {
                        if let Err(marker_error) = mark_recovery_applied(record) {
                            match recovery_record_data.as_ref().map(|data| self.recover_migration_record(data)) {
                                Some(Ok(())) => {
                                    let _ = std::fs::remove_file(record);
                                    let _ = std::fs::remove_file(record.with_extension("applied"));
                                    "import completed, but could not record it safely; the database was restored to its previous state".to_owned()
                                }
                                Some(Err(recovery_error)) => format!("import completed, but could not finalize its recovery record ({marker_error}) or restore the database ({recovery_error}); recovery will retry on the next launch"),
                                None => format!("import completed, but could not finalize its recovery record ({marker_error})"),
                            }
                        } else {
                            let _ = std::fs::remove_file(record);
                            let _ = std::fs::remove_file(record.with_extension("applied"));
                            match safety_backup {
                                Some(path) => format!(
                                    "replaced; previous database saved at {}",
                                    path.display()
                                ),
                                None if target_db != db => format!("copied as {target_db}"),
                                None => "copied".into(),
                            }
                        }
                    } else {
                        match safety_backup {
                            Some(path) => {
                                format!("replaced; previous database saved at {}", path.display())
                            }
                            None if target_db != db => format!("copied as {target_db}"),
                            None => "copied".into(),
                        }
                    }
                }
                Err(error) => {
                    if let Some(backup) = safety_backup.as_ref() {
                        let rollback = (|| {
                            let _ = self.services.run_sql(
                                target,
                                &format!("DROP DATABASE IF EXISTS {}", mysql_ident(&db)),
                            );
                            self.services.run_sql(
                                target,
                                &format!("CREATE DATABASE {}", mysql_ident(&db)),
                            )?;
                            crate::dbbackup::load_into(&self.services, target, &db, backup)
                        })();
                        match rollback {
                            Ok(()) => {
                                if let Some(record) = recovery_record.as_ref() {
                                    if mark_recovery_applied(record).is_ok() {
                                        let _ = std::fs::remove_file(record);
                                        let _ = std::fs::remove_file(record.with_extension("applied"));
                                    }
                                }
                                format!("{error}. The previous database was restored from {}.", backup.display())
                            }
                            Err(rollback_error) => format!("{error}. Automatic recovery also failed ({rollback_error}); the safety backup is at {}.", backup.display()),
                        }
                    } else if target_db != db || !target_exists {
                        let _ = self.services.run_sql(
                            target,
                            &format!("DROP DATABASE IF EXISTS {}", mysql_ident(&target_db)),
                        );
                        error
                    } else {
                        error
                    }
                }
            };
            let _ = std::fs::remove_file(&file);
            let ok = detail == "copied"
                || detail.starts_with("replaced;")
                || detail.starts_with("copied as ");
            tracing::info!(database = %db, ok, "database migration");
            let result = crate::migrate::MigratedDb {
                name: db,
                ok,
                detail,
            };
            progress.finished(result.clone());
            results.push(result);
        }
        progress.step("Cleaning up the temporary copy", None);
        drop(session);
        Ok(results)
    }

    // --------------------------------------------------------------------- logs
}

fn mysql_ident(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

fn backup_before_replace(
    backup: impl FnOnce() -> Result<std::path::PathBuf, String>,
    replace: impl FnOnce(&std::path::Path) -> Result<(), String>,
) -> Result<std::path::PathBuf, String> {
    let backup_file = backup()?;
    replace(&backup_file)?;
    Ok(backup_file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_safety_backup_never_starts_replacement() {
        let mut replacement_started = false;
        let result = backup_before_replace(
            || Err("backup failed".into()),
            |_| {
                replacement_started = true;
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(!replacement_started);
    }

    #[test]
    fn replacement_receives_completed_backup_path() {
        let root = tempfile::tempdir().unwrap();
        let backup = root.path().join("existing.sql");
        let expected = backup.clone();
        let returned = backup_before_replace(
            || Ok(backup),
            |path| {
                assert_eq!(path, expected);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(returned, expected);
    }
}
