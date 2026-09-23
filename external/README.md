# Upstream Sources

JCode is a Git submodule at `external/jcode`, pinned to
`6819cf87acdc6cc4e2c2c321148eb59c04d82749` from the project's
[JCode fork](https://github.com/HanCheg0812/erp-agent-jcode). The original
[JCode repository](https://github.com/1jehuang/jcode) remains the upstream
source (default branch: `master`).
Its source and TypeScript SDK are included. A Rust Bridge now builds and calls its
Agent directly for DeepSeek Responses; the SDK/socket path is not integrated.
Local product patches include remote PostgreSQL storage, a restricted tool entry
point, DeepSeek reasoning-event/history preservation and parallel function-call
ordering. These patches are committed on the fork's `erp-postgres-sessions`
branch. See the
[initial integration record](../docs/plantree/plans/server-demo/deepseek-integration.md)
and [Skill integration and protocol fixes](../docs/plantree/plans/server-demo/skill-parser-integration.md).

On a fresh checkout:

```sh
git submodule update --init --recursive
npm ci
```

`npm run upstream:check` compares the pinned commit with the remote HEAD without
modifying either checkout. There is no automatic pull during builds or startup.

To adopt an upstream update, use an explicit integration branch, fetch the upstream
revision, review its changes, run the runtime/permission/recovery tests, then commit
the new submodule pointer in the parent repository. Do not overwrite local JCode
changes or let separate application instances update their source at runtime.

The maintained fork is `origin`. In a fresh checkout, add the original repository
as a read-only update source:

```sh
git -C external/jcode remote add upstream https://github.com/1jehuang/jcode.git
git -C external/jcode remote set-url --push upstream DISABLED
```

Review and merge upstream changes into an integration branch on the fork, test
them, publish the fork revision, then update the parent repository's pinned
submodule commit. No pull request to the upstream repository is part of this
workflow.

OpenViking and Cube remain independent services. Their deployment configuration and
compatibility records belong in `infra/`; vendoring their full source is unnecessary
until we need maintained source changes. Service separation does not require separate
physical servers.
