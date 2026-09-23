mod application;
mod domain;
mod handlers;
mod repository;
mod types;

pub use handlers::{
    approve, create, detail, events, list, propose, set_context, start_conversation,
};
pub use types::Page;

pub(crate) use application::{propose_tx, set_context_tx};
pub(crate) use domain::{validate_context, validate_proposal};
pub(crate) use repository::{audit, find as case};

#[cfg(test)]
mod tests;

// Scoped case projection shared with Agent case reads; caller owns transaction.
pub(crate) use repository::detail as detail_in_tx;
