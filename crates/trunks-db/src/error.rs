use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum Error {
    #[snafu(display("failed to connect to the database: {reason}"))]
    Connect { reason: String },
    #[snafu(display("migration '{name}' failed: {reason}"))]
    Migration { name: String, reason: String },
    #[snafu(display("query failed: {reason}"))]
    Query { reason: String },
    #[snafu(display("row not found"))]
    NotFound,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl From<Error> for trunks_core::Error {
    fn from(e: Error) -> Self {
        match e {
            Error::NotFound => trunks_core::Error::not_found("row"),
            other => trunks_core::Error::store(other),
        }
    }
}
