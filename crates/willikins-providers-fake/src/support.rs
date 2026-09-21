//! The generic tool-authoring helpers every fake tool's
//! [`Tool`](willikins_core::Tool) impl builds on now live in
//! `willikins_core::tool::helpers` — shared with `willikins-tools` and,
//! eventually, the live providers — and are simply re-exported here so
//! this crate's tool modules keep compiling unchanged against
//! `crate::support::*`.

pub(crate) use willikins_core::tool::helpers::{
    any_secret, conflict, exact, exact_derived_only, get, invalid, list, not_found, port,
    require_present, scalar, tool_name,
};
