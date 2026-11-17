# Local Docker Databases

You can spin up local PostgreSQL, MySQL, and MariaDB test instances using the included `docker-compose.yml`:

```bash
docker compose up -d postgres mysql mariadb
```

Only the `postgres` service is used by a live driver today. MySQL and MariaDB have no live driver
yet (see "Supported Databases" in the README), so their containers are there for the follow-up work
that adds those drivers.

### Running the live PostgreSQL tests

`tests/live_postgres_suite.rs` contains a test that runs against a real server when
`RUST_SQL_TEST_PG_HOST` is set. Without it, the test prints a skip notice and passes.

```bash
docker compose up -d postgres
RUST_SQL_TEST_PG_HOST=127.0.0.1 RUST_SQL_TEST_PG_PORT=5432 \
RUST_SQL_TEST_PG_USER=postgres RUST_SQL_TEST_PG_PASSWORD=password RUST_SQL_TEST_PG_DB=rust_sql \
  cargo test --test live_postgres_suite
```


### Running the SSH tunnel tests

Unit tests in `src/core/tunnel/` run an in-process SSH server and need nothing extra. The
end-to-end test in `tests/ssh_tunnel_suite.rs` uses a real OpenSSH bastion (`ssh-bastion`) and a
PostgreSQL server on an internal network that only the bastion can reach (`postgres-private`).
Without `RUST_SQL_TEST_SSH_HOST`, the test prints a skip notice and passes.

```bash
docker compose up -d ssh-bastion postgres-private
RUST_SQL_TEST_SSH_HOST=127.0.0.1 RUST_SQL_TEST_SSH_PORT=2222 \
RUST_SQL_TEST_SSH_USER=tunnel RUST_SQL_TEST_SSH_PASSWORD=tunnel-password \
RUST_SQL_TEST_SSH_DB_HOST=postgres-private RUST_SQL_TEST_SSH_DB_PORT=5432 \
RUST_SQL_TEST_PG_USER=postgres RUST_SQL_TEST_PG_PASSWORD=password RUST_SQL_TEST_PG_DB=rust_sql \
  cargo test --test ssh_tunnel_suite
```

`postgres-private` publishes no port and its hostname only resolves inside the compose network, so
a passing test proves the query went through the bastion.
