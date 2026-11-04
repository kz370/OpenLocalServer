use crate::app::*;
use crate::error::CoreError;

impl Inner {
    fn migration_source(&self, id: &str) -> Result<crate::migrate::MigrationSource, CoreError> {
        if let Some(source) = crate::migrate::detect_managed(&self.paths, &self.runtimes)
            .into_iter()
            .find(|s| s.id == id)
        {
            return Ok(source);
        }
        crate::migrate::inspect_data_dir(std::path::Path::new(id)).map_err(svc)
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
        let result = self.migrate_databases_inner(source_id, password, databases, target, None);
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
        let result = self.migrate_databases_inner(
            source_id,
            password,
            databases,
            target,
            Some(conflict_policy),
        );
        self.migration.end();
        result
    }

    fn migrate_databases_inner(
        &self,
        source_id: &str,
        password: &str,
        databases: &[String],
        target: &str,
        conflict_policy: Option<crate::migrate::ConflictPolicy>,
    ) -> Result<Vec<crate::migrate::MigratedDb>, CoreError> {
        let progress = &self.migration;
        let source = self.migration_source(source_id)?;
        let (client, port) = self.services.sql_client(target).map_err(svc)?;
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
        let mut target_names = if conflict_policy.is_some() {
            self.services.list_databases(target).map_err(svc)?
        } else {
            Vec::new()
        };
        let total = names.len();
        for (index, db) in names.into_iter().enumerate() {
            progress.database(index + 1, total, &db);
            let resolution = conflict_policy
                .map(|policy| crate::migrate::resolve_conflict(&db, &target_names, policy))
                .unwrap_or(crate::migrate::ConflictResolution::UseIncomingName);
            if resolution == crate::migrate::ConflictResolution::Skip {
                let result = crate::migrate::MigratedDb {
                    name: db,
                    ok: false,
                    detail: "Skipped because a database with this name already exists in the target.".into(),
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
            let outcome = session.dump(&db, &file, progress).and_then(|()| {
                if matches!(&resolution, crate::migrate::ConflictResolution::Rename(_)) {
                    crate::migrate::rename_dump_database(&file, &target_db)?;
                    self.services
                        .run_sql(target, &format!("CREATE DATABASE {}", mysql_ident(&target_db)))
                        .map_err(|e| format!("could not create renamed database {target_db}: {e}"))?;
                }
                if target_exists {
                    let backup = crate::dbbackup::backup(&self.services, &self.paths, target, &db)
                        .map_err(|e| format!("could not make a safety backup, so {db} was not changed ({e})"))?;
                    safety_backup = Some(backup);
                    self.services
                        .run_sql(target, &format!("DROP DATABASE {}", mysql_ident(&db)))
                        .map_err(|e| format!("could not remove the existing {db}; its safety backup is at {} ({e})", safety_backup.as_ref().unwrap().display()))?;
                }
                progress.step(
                    format!("Importing {db} as {target_db} into {target}"),
                    std::fs::metadata(&file).ok().map(|m| m.len()),
                );
                crate::migrate::import(&client, port, &file, progress)
            });

            let detail = match outcome {
                Ok(()) => {
                    target_names.push(target_db.clone());
                    match safety_backup {
                        Some(path) => format!("replaced; previous database saved at {}", path.display()),
                        None if target_db != db => format!("copied as {target_db}"),
                        None => "copied".into(),
                    }
                }
                Err(error) => {
                    if let Some(backup) = safety_backup.as_ref() {
                        let rollback = (|| {
                            let _ = self.services.run_sql(target, &format!("DROP DATABASE IF EXISTS {}", mysql_ident(&db)));
                            self.services.run_sql(target, &format!("CREATE DATABASE {}", mysql_ident(&db)))?;
                            crate::dbbackup::load_into(&self.services, target, &db, backup)
                        })();
                        match rollback {
                            Ok(()) => format!("{error}. The previous database was restored from {}.", backup.display()),
                            Err(rollback_error) => format!("{error}. Automatic recovery also failed ({rollback_error}); the safety backup is at {}.", backup.display()),
                        }
                    } else if target_db != db {
                        let _ = self.services.run_sql(target, &format!("DROP DATABASE IF EXISTS {}", mysql_ident(&target_db)));
                        error
                    } else {
                        error
                    }
                }
            };
            let _ = std::fs::remove_file(&file);
            let ok = detail == "copied" || detail.starts_with("replaced;") || detail.starts_with("copied as ");
            tracing::info!(database = %db, ok, "database migration");
            let result = crate::migrate::MigratedDb { name: db, ok, detail };
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
