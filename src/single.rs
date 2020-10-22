//! A single connection abstraction to a SQL database.

use crate::{
    ast,
    connector::{self, ConnectionInfo, Queryable, TransactionCapable, DEFAULT_SQLITE_SCHEMA_NAME},
};
use async_trait::async_trait;
use std::{fmt, sync::Arc};
use url::Url;

#[cfg(feature = "sqlite")]
use std::convert::TryFrom;

/// The main entry point and an abstraction over a database connection.
#[derive(Clone)]
pub struct Quaint {
    inner: Arc<dyn Queryable>,
    connection_info: Arc<ConnectionInfo>,
}

impl fmt::Debug for Quaint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.connection_info)
    }
}

impl TransactionCapable for Quaint {}

impl Quaint {
    /// Create a new connection to the database. The connection string
    /// follows the specified format:
    ///
    /// `connector_type://user:password@host/database?parameters`
    ///
    /// Connector type can be one of the following:
    ///
    /// - `sqlite`/`file` opens an SQLite connection
    /// - `mysql` opens a MySQL connection
    /// - `postgres`/`postgresql` opens a PostgreSQL connection
    ///
    /// All parameters should be given in the query string format:
    /// `?key1=val1&key2=val2`. All parameters are optional.
    ///
    /// SQLite:
    ///
    /// - `user`/`password` do not do anything and can be emitted.
    /// - `host` should point to the database file.
    /// - `db_name` parameter should give a name to the database attached for
    ///   query namespacing.
    /// - `socket_timeout` defined in seconds. Acts as the busy timeout in
    ///   SQLite. When set, queries that are waiting for a lock to be released
    ///   will return the `Timeout` error after the defined value.
    ///
    /// PostgreSQL:
    ///
    /// - `sslmode` either `disable`, `prefer` or `require`. [Read more](https://docs.rs/tokio-postgres/0.5.0-alpha.1/tokio_postgres/config/enum.SslMode.html)
    /// - `sslcert` should point to a PEM certificate file.
    /// - `sslidentity` should point to a PKCS12 certificate database.
    /// - `sslpassword` the password to open the PKCS12 database.
    /// - `sslaccept` either `strict` or `accept_invalid_certs`. If strict, the
    ///   certificate needs to be valid and in the CA certificates.
    ///   `accept_invalid_certs` accepts any certificate from the server and can
    ///   lead to weakened security. Defaults to `strict`.
    /// - `schema` the default search path.
    /// - `host` additionally the host can be given as a parameter, typically in
    ///   cases when connectiong to the database through a unix socket to
    ///   separate the database name from the database path, such as
    ///   `postgresql:///dbname?host=/var/run/postgresql`.
    /// - `socket_timeout` defined in seconds. If set, a query will return a
    ///   `Timeout` error if it fails to resolve before given time.
    /// - `connect_timeout` defined in seconds (default: 5). Connecting to a
    ///   database will return a `ConnectTimeout` error if taking more than the
    ///   defined value.
    /// - `pgbouncer` either `true` or `false`. If set, allows usage with the
    ///   pgBouncer connection pool in transaction mode. Additionally a transaction
    ///   is required for every query for the mode to work. When starting a new
    ///   transaction, a deallocation query `DEALLOCATE ALL` is executed right after
    ///   `BEGIN` to avoid possible collisions with statements created in other
    ///   sessions.
    /// - `statement_cache_size`, number of prepared statements kept cached.
    ///   Defaults to 500, which means caching is off. If `pgbouncer` mode is enabled,
    ///   caching is always off.
    ///
    /// MySQL:
    ///
    /// - `sslcert` should point to a PEM certificate file.
    /// - `sslidentity` should point to a PKCS12 certificate database.
    /// - `sslpassword` the password to open the PKCS12 database.
    /// - `sslaccept` either `strict` or `accept_invalid_certs`. If strict, the
    ///   certificate needs to be valid and in the CA certificates.
    ///   `accept_invalid_certs` accepts any certificate from the server and can
    ///   lead to weakened security. Defaults to `strict`.
    /// - `socket` needed when connecting to MySQL database through a unix
    ///   socket. When set, the host parameter is dismissed.
    /// - `socket_timeout` defined in seconds. If set, a query will return a
    ///   `Timeout` error if it fails to resolve before given time.
    /// - `connect_timeout` defined in seconds (default: 5). Connecting to a
    ///   database will return a `ConnectTimeout` error if taking more than the
    ///   defined value.
    ///
    /// Microsoft SQL Server:
    ///
    /// - `encrypt` if set to `true` encrypts all traffic over TLS. If `false`, only
    ///   the login details are encrypted. A special value `DANGER_PLAINTEXT` will
    ///   disable TLS completely, including sending login credentials as plaintext.
    /// - `user` sets the login name.
    /// - `password` sets the login password.
    /// - `database` sets the database to connect to.
    /// - `trustServerCertificate` if set to `true`, accepts any kind of certificate
    ///   from the server.
    /// - `socketTimeout` defined in seconds. If set, a query will return a
    ///   `Timeout` error if it fails to resolve before given time.
    /// - `connectTimeout` defined in seconds (default: 5). Connecting to a
    ///   database will return a `ConnectTimeout` error if taking more than the
    ///   defined value.
    /// - `connectionLimit` defines the maximum number of connections opened to the
    ///   database.
    /// - `schema` the name of the lookup schema. Only stored to the connection,
    ///   must be used in every query to be effective.
    /// - `isolationLevel` the transaction isolation level. Possible values:
    ///   `READ UNCOMMITTED`, `READ COMMITTED`, `REPEATABLE READ`, `SNAPSHOT`,
    ///   `SERIALIZABLE`.
    pub async fn new(url_str: &str) -> crate::Result<Self> {
        let inner = match url_str {
            #[cfg(feature = "sqlite")]
            s if s.starts_with("file") || s.starts_with("sqlite") => {
                let params = connector::SqliteParams::try_from(s)?;
                let mut sqlite = connector::Sqlite::new(&params.file_path)?;

                sqlite.attach_database(&params.db_name).await?;

                Arc::new(sqlite) as Arc<dyn Queryable>
            }
            #[cfg(feature = "mysql")]
            s if s.starts_with("mysql") => {
                let url = connector::MysqlUrl::new(Url::parse(s)?)?;
                let mysql = connector::Mysql::new(url)?;

                Arc::new(mysql) as Arc<dyn Queryable>
            }
            #[cfg(feature = "postgresql")]
            s if s.starts_with("postgres") || s.starts_with("postgresql") => {
                let url = connector::PostgresUrl::new(Url::parse(s)?)?;
                let psql = connector::PostgreSql::new(url).await?;
                Arc::new(psql) as Arc<dyn Queryable>
            }
            #[cfg(feature = "mssql")]
            s if s.starts_with("jdbc:sqlserver") || s.starts_with("sqlserver") => {
                let url = connector::MssqlUrl::new(s)?;
                let psql = connector::Mssql::new(url).await?;

                Arc::new(psql) as Arc<dyn Queryable>
            }
            _ => unimplemented!("Supported url schemes: file or sqlite, mysql, postgresql or sqlserver."),
        };

        let connection_info = Arc::new(ConnectionInfo::from_url(url_str)?);
        Self::log_start(&connection_info);

        Ok(Self { inner, connection_info })
    }

    #[cfg(feature = "sqlite")]
    /// Open a new SQLite database in memory.
    pub fn new_in_memory(attached_name: Option<String>) -> crate::Result<Quaint> {
        let attached_name = attached_name.unwrap_or_else(|| DEFAULT_SQLITE_SCHEMA_NAME.into());

        Ok(Quaint {
            inner: Arc::new(connector::Sqlite::new_in_memory(attached_name.clone())?),
            connection_info: Arc::new(ConnectionInfo::InMemorySqlite { db_name: attached_name }),
        })
    }

    /// Info about the connection and underlying database.
    pub fn connection_info(&self) -> &ConnectionInfo {
        &self.connection_info
    }

    fn log_start(info: &ConnectionInfo) {
        let family = info.sql_family();
        let pg_bouncer = if info.pg_bouncer() { " in PgBouncer mode" } else { "" };

        #[cfg(not(feature = "tracing-log"))]
        {
            info!("Starting a {} connection{}.", family, pg_bouncer);
        }
        #[cfg(feature = "tracing-log")]
        {
            tracing::info!("Starting a {} connection{}.", family, pg_bouncer);
        }
    }
}

#[async_trait]
impl Queryable for Quaint {
    async fn query(&self, q: ast::Query<'_>) -> crate::Result<connector::ResultSet> {
        self.inner.query(q).await
    }

    async fn execute(&self, q: ast::Query<'_>) -> crate::Result<u64> {
        self.inner.execute(q).await
    }

    async fn query_raw(&self, sql: &str, params: &[ast::Value<'_>]) -> crate::Result<connector::ResultSet> {
        self.inner.query_raw(sql, params).await
    }

    async fn execute_raw(&self, sql: &str, params: &[ast::Value<'_>]) -> crate::Result<u64> {
        self.inner.execute_raw(sql, params).await
    }

    async fn raw_cmd(&self, cmd: &str) -> crate::Result<()> {
        self.inner.raw_cmd(cmd).await
    }

    async fn version(&self) -> crate::Result<Option<String>> {
        self.inner.version().await
    }

    fn begin_statement(&self) -> &'static str {
        self.inner.begin_statement()
    }
}
