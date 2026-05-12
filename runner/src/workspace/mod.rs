pub mod context;
pub mod git;
pub mod resolve;

pub use context::{ContextFields, write_context_md};
pub use resolve::{Resolution, ResolveError, resolve};
