mod documents;
mod handlers;
mod leases;
mod receipts;
mod registration;
mod storage;

pub use handlers::{claim_route, create, document_get, document_put, load, save};
pub(crate) use leases::authorize;
pub(crate) use registration::{bind, claim};
