//! Controlled language pack policy and build module (Milestone 4A.2).

pub mod builder;
pub mod collisions;
pub mod manifest;
pub mod policy;
pub mod selection;

pub use builder::build_pack;
pub use policy::{PackPolicyConfig, PACK_POLICY_SCHEMA_VERSION};
