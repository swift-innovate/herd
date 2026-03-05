pub mod admin;
pub mod openai;
pub mod status;

// Re-export handlers for convenience
pub use admin::{list_backends, add_backend, get_backend, update_backend, remove_backend};