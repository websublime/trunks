use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum Error {
    #[snafu(display("unsupported database URL '{url}': expected sqlite:// or postgres://"))]
    UnsupportedUrl { url: String },
    #[snafu(display("failed to connect to {backend}: {source}"))]
    Connect { backend: &'static str, source: sqlx::Error },
    #[snafu(display("migration failed: {source}"))]
    Migrate { source: sqlx::migrate::MigrateError },
    #[snafu(display("query failed: {source}"))]
    Query { source: sqlx::Error },
    #[snafu(display("row not found"))]
    NotFound,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl From<sqlx::Error> for Error {
    fn from(source: sqlx::Error) -> Self {
        match source {
            sqlx::Error::RowNotFound => Self::NotFound,
            source => Self::Query { source },
        }
    }
}

impl From<Error> for trunks_core::Error {
    fn from(e: Error) -> Self {
        match e {
            Error::NotFound => trunks_core::Error::not_found("row"),
            other => trunks_core::Error::store(other),
        }
    }
}
