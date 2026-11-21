#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("cannot connect to the database: {source}")]
    Connect {
        #[source]
        source: sqlx::Error,
    },
    #[error(transparent)]
    Migration(#[from] migrator::Error),
    #[error(
        "the database schema is behind this build, missing migrations: {}; run `webhooks-live migrate up`",
        .missing.join(", ")
    )]
    SchemaBehind { missing: Vec<String> },
}
