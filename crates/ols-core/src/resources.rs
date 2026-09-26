//! Resource controls (§129). Optional limits, all off by default:
//!
//! - database / cache memory, passed to each server as its own setting when it starts
//!   (MariaDB's buffer pool, PostgreSQL's shared buffers, Redis' maxmemory, MongoDB's cache);
//! - Node's heap size, through `NODE_OPTIONS` for every project command;
//! - how many copies one queue worker may run, and how many processes the app may start.
//!
//! Platform limits are respected rather than faked: Windows has no simple per-process CPU
//! cap without Job Objects, so there is no CPU setting. Changes apply the next time a
//! service starts; the UI says so.

use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::error::CoreError;

const KEY: &str = "resources";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResourceLimits {
    #[serde(default)]
    pub mariadb_buffer_pool_mb: Option<u32>,
    #[serde(default)]
    pub postgres_shared_buffers_mb: Option<u32>,
    #[serde(default)]
    pub redis_maxmemory_mb: Option<u32>,
    #[serde(default)]
    pub mongodb_cache_mb: Option<u32>,
    #[serde(default)]
    pub node_max_old_space_mb: Option<u32>,
    #[serde(default)]
    pub max_worker_count: Option<u32>,
    /// Processes the app itself may run at once (services, sites, workers, commands).
    #[serde(default)]
    pub max_processes: Option<u32>,
    /// The most virtual users a load-test script may ask for (default 200).
    #[serde(default)]
    pub k6_max_vus: Option<u32>,
}

impl ResourceLimits {
    pub fn validate(&self) -> Result<(), String> {
        let check = |v: Option<u32>, min: u32, max: u32, what: &str| match v {
            Some(v) if v < min || v > max => Err(format!("{what} must be between {min} and {max}")),
            _ => Ok(()),
        };
        check(self.mariadb_buffer_pool_mb, 16, 65536, "The MariaDB buffer pool (MB)")?;
        check(self.postgres_shared_buffers_mb, 16, 65536, "PostgreSQL shared buffers (MB)")?;
        check(self.redis_maxmemory_mb, 8, 65536, "Redis memory (MB)")?;
        check(self.mongodb_cache_mb, 256, 65536, "The MongoDB cache (MB)")?;
        check(self.node_max_old_space_mb, 64, 65536, "Node memory (MB)")?;
        check(self.max_worker_count, 1, crate::workers::MAX_COUNT, "Copies per worker")?;
        check(self.max_processes, 4, 1000, "The process limit")?;
        check(self.k6_max_vus, 1, 5000, "The load-test virtual users")?;
        Ok(())
    }

    /// Extra command-line arguments for a service, from these limits.
    pub fn service_args(&self, id: &str) -> Vec<String> {
        match id {
            "mariadb" => self.mariadb_buffer_pool_mb.map(|m| vec![format!("--innodb-buffer-pool-size={m}M")]).unwrap_or_default(),
            "postgres" => self.postgres_shared_buffers_mb.map(|m| vec!["-c".into(), format!("shared_buffers={m}MB")]).unwrap_or_default(),
            "redis" => self.redis_maxmemory_mb.map(|m| vec!["--maxmemory".into(), format!("{m}mb"), "--maxmemory-policy".into(), "allkeys-lru".into()]).unwrap_or_default(),
            "mongodb" => self.mongodb_cache_mb.map(|m| vec!["--wiredTigerCacheSizeGB".into(), format!("{:.2}", (m as f64 / 1024.0).max(0.25))]).unwrap_or_default(),
            _ => Vec::new(),
        }
    }
}

impl Inner {
    pub fn resource_limits(&self) -> ResourceLimits {
        self.settings.lock().unwrap().get(KEY).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
    }

    pub fn set_resource_limits(&self, limits: ResourceLimits) -> Result<(), CoreError> {
        limits.validate().map_err(CoreError::ServiceError)?;
        self.settings.lock().unwrap().set(KEY.to_string(), serde_json::to_value(&limits)?)?;
        self.services.set_limits(limits);
        Ok(())
    }

    /// Room for one more process under the process limit.
    pub fn process_slot_free(&self) -> bool {
        match self.resource_limits().max_processes {
            Some(max) => (self.supervisor.snapshot().into_iter().filter(|p| self.supervisor.is_alive(p.id)).count() as u32) < max,
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_become_each_servers_own_flags() {
        let l = ResourceLimits { mariadb_buffer_pool_mb: Some(256), redis_maxmemory_mb: Some(64), mongodb_cache_mb: Some(256), ..Default::default() };
        assert_eq!(l.service_args("mariadb"), ["--innodb-buffer-pool-size=256M"]);
        assert_eq!(l.service_args("redis")[..2], ["--maxmemory".to_string(), "64mb".to_string()]);
        assert_eq!(l.service_args("mongodb")[1], "0.25");
        assert!(l.service_args("postgres").is_empty(), "unset limits add nothing");
    }

    #[test]
    fn silly_values_are_refused() {
        assert!(ResourceLimits { redis_maxmemory_mb: Some(1), ..Default::default() }.validate().is_err());
        assert!(ResourceLimits { max_worker_count: Some(99), ..Default::default() }.validate().is_err());
        assert!(ResourceLimits::default().validate().is_ok());
    }
}
