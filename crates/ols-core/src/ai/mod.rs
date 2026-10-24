//! The AI assistant (Stage 19). Opt-in, off by default, bring your own model: OLS ships no model and no
//! account. Every provider (LM Studio, Ollama, Hugging Face, OpenRouter, any OpenAI-compatible server) is the same
//! code over the chat-completions API with a different base URL and key.

pub mod chat;
pub mod client;
pub mod policy;
pub mod providers;
pub mod tools;
pub mod types;

#[cfg(test)]
mod tests;

pub use self::chat::*;
pub use self::client::*;
pub use self::policy::*;
pub use self::providers::*;
pub use self::tools::*;
pub use self::types::*;
