use crate::app::system::*;
use crate::app::*;
use crate::dbtools;
use crate::domain::Domain;
use crate::error::CoreError;
use crate::health::{HealthReport, HealthTarget};
use std::path::{Path, PathBuf};

impl Inner {
    fn migration_source(&self, id: &str) -> Result<crate::migrate::MigrationSource, CoreError> {
        crate::migrate::detect()
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| svc(format!("{id} is no longer there")))
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
        let result = self.migrate_databases_inner(source_id, password, databases, target);
        self.migration.end();
        result
    }

    fn migrate_databases_inner(
        &self,
        source_id: &str,
        password: &str,
        databases: &[String],
        target: &str,
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
        let total = names.len();
        for (index, db) in names.into_iter().enumerate() {
            progress.database(index + 1, total, &db);
            let file = work.join(format!("migrate-{}.sql", crate::domain::slugify(&db)));
            progress.step(
                format!("Exporting {db}"),
                sizes.get(&db).copied().filter(|s| *s > 0),
            );
            let outcome = session.dump(&db, &file, progress).and_then(|()| {
                progress.step(
                    format!("Importing {db} into {target}"),
                    std::fs::metadata(&file).ok().map(|m| m.len()),
                );
                crate::migrate::import(&client, port, &file, progress)
            });
            let _ = std::fs::remove_file(&file);
            tracing::info!(database = %db, ok = outcome.is_ok(), "database migration");
            let result = match outcome {
                Ok(()) => crate::migrate::MigratedDb {
                    name: db,
                    ok: true,
                    detail: "copied".into(),
                },
                Err(e) => crate::migrate::MigratedDb {
                    name: db,
                    ok: false,
                    detail: e,
                },
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
