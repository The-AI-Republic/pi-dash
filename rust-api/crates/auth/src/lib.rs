#![forbid(unsafe_code)]

//! Authentication and authorization primitives.
//!
//! The Django session reader (F-05) and the permission kernel (F-06) build on
//! the tenant scope defined here: every checked access names the workspace it
//! acts in, and anything without a scope is denied.

pub mod scope;

pub use scope::TenantScope;
