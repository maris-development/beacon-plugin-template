# beacon-plugin-template

A template for a [Beacon](https://github.com/maris-development/beacon) function plugin.
A plugin adds scalar SQL functions to Beacon.
This template holds a native Rust crate, a SQL function, a package script and a release workflow.

Read the Beacon guide `docs/docs/2.1.0-rc1/guides/write-a-plugin.md` for the full rules.

## Contents

| Path | Contents |
| --- | --- |
| `beacon-plugin.toml` | The plugin manifest: name, version, supported Beacon versions and files. |
| `native/` | A Rust `cdylib` crate on `beacon-plugin-sdk`. It holds the native functions and unit tests. |
| `native/Cargo.lock` | The lock file. The package script reads the DataFusion version from it. |
| `sql/functions.sql` | `CREATE FUNCTION` statements. They can call the native functions. |
| `scripts/package-plugin.py` | Packs the plugin into an archive and writes `SHA256SUMS`. |
| `.github/workflows/release.yml` | Builds and packs the plugin for a tag. Attaches the archives to the GitHub release. |

The example plugin `example` has four functions:

| Function | Type | Result |
| --- | --- | --- |
| `example_add_one(x)`, alias `example_inc(x)` | Native, closure helper | `x + 1` |
| `example_hypot(x, y)` | Native, `ScalarUDFImpl` | `sqrt(x² + y²)` |
| `example_in_box(lon, lat, min_lon, min_lat, max_lon, max_lat)` | Native, closure helper with a planner hook | `true` when the point is in the box |
| `example_add_two(x)` | SQL | `x + 2` |

## Use the template

1. Click **Use this template** on GitHub. Make a new repository.
2. Clone the new repository.
3. Rename the plugin. Change the names in the table below.
4. Write your functions in `native/src/lib.rs` and `sql/functions.sql`.

| File | Change |
| --- | --- |
| `beacon-plugin.toml` | `plugin.name`, `plugin.version` and `native.library`. |
| `native/Cargo.toml` | `package.name` and `lib.name`. `lib.name` must equal `native.library`. |
| `native/src/lib.rs` | The function names. Start each name with the plugin name. |
| `sql/functions.sql` | The function names. |

The plugin name has lowercase letters, digits and `_` only. It starts with a letter.
A SQL-only plugin needs no Rust code. Remove `[native]` from the manifest and remove `native/`.

## Choose the Beacon version

`native/Cargo.toml` gets `beacon-plugin-sdk` from the Beacon Git repository.
The Beacon ref fixes the DataFusion version of the plugin.
Beacon refuses a plugin with a different DataFusion major version.

The template uses the branch `main`, because no Beacon release holds the SDK yet.
Set a Beacon release tag when one is available:

```toml
beacon-plugin-sdk = { git = "https://github.com/maris-development/beacon", tag = "v2.1.0" }
```

Then run `cargo update -p beacon-plugin-sdk` in `native/`.
Make `plugin.beacon` in `beacon-plugin.toml` match the Beacon versions of that DataFusion version.

Use the `datafusion` crate that the SDK re-exports. Do not add `datafusion` as a dependency.
Keep `panic = "unwind"`. This is the default. The SDK refuses `panic = "abort"`.

## Add planner hooks

DataFusion cannot see into a plugin function. So a filter on the function prunes no files.
A planner hook tells Beacon what a call means. All hooks are optional.

| Builder method | Use |
| --- | --- |
| `implies(f)` | A plain predicate that the call implies. |
| `bounds(f)` | The output interval for the argument intervals. |
| `propagate(f)` | The argument intervals for an output interval. |

`example_in_box` uses `implies`. The filter `example_in_box(lon, lat, 2, 50, 8, 56)` implies
`lon BETWEEN 2 AND 8 AND lat BETWEEN 50 AND 56`. Beacon uses that predicate to prune files.

- `Implied::Necessary(expr)`: Beacon adds `expr` next to the call. The rule must be correct. Else rows disappear from the result.
- `Implied::Exact(expr)`: Beacon replaces the call with `expr`.
- A hook that gives `Ok(None)`, an error or a panic gives no answer. The query does not fail.
- For a `ScalarUDFImpl`, implement `FunctionHooks` and wrap the function with `HookedUdf::new`.

The SDK README in the Beacon repository has more detail.

## Run the unit tests

The unit tests run the native functions in DataFusion. They need no Beacon server.

```bash
cargo test --manifest-path native/Cargo.toml
```

## Pack the plugin

Do these steps on Linux. The Beacon server image is a Linux image, so the library must be a Linux `.so` file.
On Windows, use WSL.

1. Build the library:

   ```bash
   cargo build --release --manifest-path native/Cargo.toml
   ```

2. Pack the plugin. The script writes the archive and `SHA256SUMS` in `dist/`:

   ```bash
   python3 scripts/package-plugin.py . --out dist --library native/target/release/libexample_native.so --target x86_64-unknown-linux-gnu
   ```

| Plugin type | Archive name |
| --- | --- |
| native | `<name>-<version>-df<major>-<target>.tar.gz` |
| SQL only | `<name>-<version>-any.tar.gz` |

To try the archive, mount `dist/` into a Beacon container as `/plugins`. Then run:

```sql
INSTALL example FROM 'file:///plugins/example-0.1.0-df54-x86_64-unknown-linux-gnu.tar.gz'
```

## Release the plugin

1. Set `plugin.version` in `beacon-plugin.toml`.
2. Commit the change.
3. Push a tag `v<version>`:

   ```bash
   git tag v0.1.0
   ```

   ```bash
   git push origin v0.1.0
   ```

The workflow `release.yml` stops if the tag does not match `plugin.version`.
It builds the library for `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`.
Then it attaches the archives and one `SHA256SUMS` file to the release `v0.1.0`.
The workflow runs no tests. Run the unit tests before you push the tag.

## Install the plugin

An admin installs the plugin with SQL. Replace `<owner>/<repo>` with your repository:

```sql
INSTALL example FROM 'github:<owner>/<repo>@v0.1.0'
```

Beacon reads `SHA256SUMS` from the release and checks the archive.
It picks the archive for its own DataFusion version and target.

To install the plugin at server start, set `BEACON_PLUGINS`:

```bash
BEACON_PLUGINS=example=github:<owner>/<repo>@v0.1.0
```

`beacon.system.plugins` shows the installed plugins. `UNINSTALL example` removes the plugin.

## Rules for native code

- A fault in native code stops the Beacon server. Test the plugin well.
- A panic in a function becomes a query error.
- Do not panic in `name`, `aliases`, `signature` or `Drop`.

## License

Apache-2.0. See [LICENSE](LICENSE).
