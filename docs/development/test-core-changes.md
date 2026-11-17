# Testing Core Engine & Adapter Changes

All core engine and database adapter tests live in `src/` unit tests and the suites in `tests/`.
Live PostgreSQL tests need a server; see [docker-databases.md](docker-databases.md).

```bash
cargo test
```
