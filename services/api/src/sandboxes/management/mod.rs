mod domain;
mod handlers;
mod service;

pub use handlers::{
    activate_candidate, adopt_template, build_candidate, candidate, create_canary,
    create_candidate, overview,
};
pub(super) use service::{Selection, selection, selection_for_template};
