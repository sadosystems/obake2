pub use env_var::{EnvVar, bool_env_var, env_var};
use std::sync::LazyLock;

/// Whether Obake is running in stateless mode.
/// When true, Obake will use in-memory databases instead of persistent storage.
pub static OBAKE_STATELESS: LazyLock<bool> = bool_env_var!("OBAKE_STATELESS");
