# SQL migration naming

Migrations use `YYYYMMDDHHmmss_<change_description>.sql`: a UTC timestamp to
second precision followed by a lowercase snake_case description. The existing
history now follows this convention, for example
`20260922020919_create_skill_catalogs.sql`. Do not use sequential counters.

## Adding a migration

1. Generate the prefix with `date -u +%Y%m%d%H%M%S`. Check for an existing prefix;
   if one already exists, wait for the next second and regenerate it.
2. Describe the actual change, such as `create_document_jobs` or `add_case_owner_index`.
3. Register the SQL in `src/platform/service.rs` using `include_str!`. Use the filename
   without `.sql` as the new migration's persistent ID.
4. Append the entry after its dependencies in `MIGRATIONS`. The runner uses this
   explicit list order, not filesystem or timestamp sorting. Resolve branch-order
   conflicts before applying migrations; timestamps alone do not establish dependencies.
5. Run `npm run check` against the local PostgreSQL container.

## Renamed history

The former `001`–`018` files and registry IDs were intentionally renamed as a
single migration-history rewrite. Their SQL contents and execution order are
unchanged. The migration runner verifies the old ID's checksum and atomically
renames it to the new timestamped ID before applying pending migrations, so an
existing database keeps its data and does not execute these SQL files again.
Applied migrations remain immutable after this rewrite; future corrections
require a new timestamped migration.

This convention applies to our business API, not the pinned `external/jcode` submodule.
