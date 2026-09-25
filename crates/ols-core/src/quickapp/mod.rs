//! Quick Apps and Quick Commands (§79–93, Stage 7): editable YAML recipes that turn a few
//! answers into a working project — runtimes installed, files written, database created,
//! domain + certificate + web server configured, health checked.
//!
//! * `schema` — the YAML definition and its validation
//! * `plan`   — answers + definition → a reviewable plan (nothing executes yet)
//! * `catalog`— built-in / local / imported definitions, favorites, trust
//! * `run`    — executes a plan step by step through the controlled command runner

pub mod catalog;
pub mod commands;
pub mod plan;
pub mod run;
pub mod schema;

pub use catalog::{EntryDetail, EntrySource, EntryView, QuickCatalog};
pub use plan::{build_plan, resolve_values, FieldError, PlanCtx, RunPlan};
pub use run::{QuickHost, RunManager, RunView};
pub use schema::QuickApp;
