use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("task not found: {0}")]
    TaskNotFound(String),

    #[error("tag not found: {0}")]
    TagNotFound(String),

    #[error("invalid status transition: {from} -> {to}")]
    InvalidTransition { from: String, to: String },
}
