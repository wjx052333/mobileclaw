pub mod client;
pub mod types;
pub mod provider;
pub mod openai_compat;  // declare now, implement next task
pub mod ollama;         // declare now, implement later
pub mod probe;          // declare now, implement later
pub use types::{ContentBlock, Message, Role, StreamEvent};
