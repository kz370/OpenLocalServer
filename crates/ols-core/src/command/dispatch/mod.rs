pub mod database;
pub mod domains;
pub mod maintenance;
pub mod ops;
pub mod services;
pub mod system;
pub mod tools;
pub mod web;

use super::types::*;
use super::Core;
use crate::error::CoreError;

impl Core {
    pub(crate) fn dispatch_inner(
        &self,
        mut command: CoreCommand,
    ) -> Result<CoreResponse, CoreError> {
        command = match self.dispatch_system(command) {
            Ok(res) => return res,
            Err(cmd) => cmd,
        };
        command = match self.dispatch_services(command) {
            Ok(res) => return res,
            Err(cmd) => cmd,
        };
        command = match self.dispatch_domains(command) {
            Ok(res) => return res,
            Err(cmd) => cmd,
        };
        command = match self.dispatch_web(command) {
            Ok(res) => return res,
            Err(cmd) => cmd,
        };
        command = match self.dispatch_database(command) {
            Ok(res) => return res,
            Err(cmd) => cmd,
        };
        command = match self.dispatch_maintenance(command) {
            Ok(res) => return res,
            Err(cmd) => cmd,
        };
        command = match self.dispatch_ops(command) {
            Ok(res) => return res,
            Err(cmd) => cmd,
        };
        match self.dispatch_tools(command) {
            Ok(res) => res,
            Err(cmd) => Err(CoreError::ServiceError(format!(
                "Unhandled command: {cmd:?}"
            ))),
        }
    }
}
