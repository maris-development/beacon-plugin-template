# beacon-plugin-template

A template for a [Beacon](https://github.com/maris-development/beacon) plugin.
A plugin adds scalar SQL functions and file formats to Beacon.
This template holds a native Rust crate, a SQL function, a package script and a release workflow.

The [template wiki](https://github.com/maris-development/beacon-plugin-template/wiki) is the full guide: functions, planner hooks, file formats, testing, releases, installs and troubleshooting.

The Beacon repository has the full rules:

- [SDK README](https://github.com/maris-development/beacon/blob/main/beacon-db/beacon-plugin-sdk/README.md): functions, planner hooks, file formats and object stores.
- [Registry README](https://github.com/maris-development/beacon/blob/main/beacon-db/beacon-plugin-registry/README.md): the manifest, the archives and the install sources.

## Contents

| Path | Contents |
| --- | --- |
| `beacon-plugin.toml` | The plugin manifest: name, version, supported Beacon versions and files. |
| `native/` | A Rust `cdylib` crate on `beacon-plugin-sdk`. It holds the native functions, the file format and the unit tests. |
| `native/src/lib.rs` | The native functions, their unit tests and the export macro. |
| `native/src/tsv.rs` | The file format `example_tsv` and its unit tests. |
| `native/testdata/stations.etsv` | A sample file for `example_tsv`. |
| `native/Cargo.lock` | The lock file. The package script reads the DataFusion version from it. |
| `sql/functions.sql` | `CREATE FUNCTION` statements. They can call the native functions. |
| `scripts/package-plugin.py` | Packs the plugin into an archive and writes `SHA256SUMS`. |
| `.github/workflows/release.yml` | Builds and packs the plugin for a tag, for Linux, macOS and Windows. Attaches the archives to the GitHub release. |

The example plugin `example` has five functions:

| Function | Type | Result |
| --- | --- | --- |
| `example_add_one(x)`, alias `example_inc(x)` | Native, closure helper with planner hooks | `x + 1` |
| `example_hypot(x, y)` | Native, `ScalarUDFImpl` | `sqrt(x² + y²)` |
| `example_in_box(lon, lat, min_lon, min_lat, max_lon, max_lat)` | Native, closure helper with a planner hook | `true` when the point is in the box |
| `example_round(x, digits)` | Native, closure helper with a constant argument | `x` rounded to `digits` decimals |
| `example_add_two(x)` | SQL | `x + 2` |

It also has one file format, `example_tsv`. See [Add a file format](#add-a-file-format).

## Use the template

1. Click **Use this template** on GitHub. Make a new repository.
2. Clone the new repository.
3. Rename the plugin. Change the names in the table below.
4. Write your functions in `native/src/lib.rs` and `sql/functions.sql`.
5. Write your file formats in `native/src/tsv.rs`, or in a new module.
6. Remove the examples that you do not need.

| File | Change |
| --- | --- |
| `beacon-plugin.toml` | `plugin.name`, `plugin.version` and `native.library`. |
| `native/Cargo.toml` | `package.name` and `lib.name`. `lib.name` must equal `native.library`. |
| `native/src/lib.rs` | The function names. Start each name with the plugin name. |
| `native/src/tsv.rs` | The format name and the extensions. Start the format name with the plugin name. |
| `sql/functions.sql` | The function names. |

The plugin name has lowercase letters, digits and `_` only. It starts with a letter.

A plugin needs only the parts that it uses:

| Plugin has | Change |
| --- | --- |
| No file format | Remove `native/src/tsv.rs`, `native/testdata/` and `mod tsv;`. Use `export_plugin!(scalar = functions)`. |
| No native function | Keep the formats. Use `export_plugin!(formats = tsv::formats)`. |
| No SQL function | Remove `[sql]` from the manifest and remove `sql/`. |
| SQL functions only | Remove `[native]` from the manifest and remove `native/`. |

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

## Read the arguments

The SDK has two builder methods for the function body:

| Builder method | Arguments |
| --- | --- |
| `invoke_arrays(f)` | Arrays only. Each constant argument becomes an array with one value per row. |
| `invoke(f)` | One `ColumnarValue` per argument. A constant argument, such as `2` in `f(x, 2)`, is a `ColumnarValue::Scalar`. |

Use `invoke` to require a constant argument, or to prepare a constant one time per batch.
`example_round` refuses a column for `digits`. It computes the factor `10^digits` one time per batch.

The body of `invoke` and of a `ScalarUDFImpl` must accept a scalar for each argument.
`example_hypot` calls `to_array` to change each argument to an array.

## Add planner hooks

DataFusion cannot see into a plugin function. So a filter on the function prunes no files.
A planner hook tells Beacon what a call means. All hooks are optional.

| Builder method | Use |
| --- | --- |
| `implies(f)` | A plain predicate that the call implies. |
| `bounds(f)` | The output interval for the argument intervals. |
| `propagate(f)` | The argument intervals for an output interval. |

The template has two examples:

- `example_in_box` uses `implies`. The filter `example_in_box(lon, lat, 2, 50, 8, 56)` implies
  `lon BETWEEN 2 AND 8 AND lat BETWEEN 50 AND 56`. Beacon uses that predicate to prune files.
- `example_add_one` uses `bounds` and `propagate`. The filter `example_add_one(x) > 10` gives `x > 9`.

Beacon combines the hooks:

- **Nested calls.** In `example_in_box(example_add_one(lon), lat, 3, 50, 9, 56)`, Beacon gets the
  range `[3, 9]` for the result of `example_add_one`. Then the `propagate` hook gives `lon` in `[2, 8]`.
  This also works when the two functions come from different plugins.
- **Arithmetic and casts.** Beacon follows `+` and `-` with a constant, `*` and `/` with a constant
  for float types, negation and casts. So `example_add_one(x + 1) > 10` gives `x > 8`.
- **Other conditions in the filter.** A condition such as `x >= 0` in the same filter sets the input
  interval of `x`. A `propagate` hook gets this interval, so it can relate two columns.
- **Other plugin functions.** The arguments and the result of `implies` can call any function
  that Beacon knows, also a function of another plugin. A function from another library is an
  FFI wrapper. Find it by its name, not with a downcast.

Rules:

- `Implied::Necessary(expr)`: Beacon adds `expr` next to the call. The rule must be correct. Else rows disappear from the result.
- `Implied::Exact(expr)`: Beacon replaces the call with `expr`. Beacon applies these rules inside the arguments before `implies` sees them.
- `bounds` must hold every value that the function can give. `propagate` must hold every input value that can give the output.
- A hook that gives `Ok(None)`, an error or a panic gives no answer. The query does not fail.
- For a `ScalarUDFImpl`, implement `FunctionHooks` and wrap the function with `HookedUdf::new`.
  `FunctionHooks::implied` gets the session `TaskContext` with all functions that Beacon knows.

The [SDK README](https://github.com/maris-development/beacon/blob/main/beacon-db/beacon-plugin-sdk/README.md) has more detail.

## Add a file format

A file format lets Beacon read a new type of file.
The example `example_tsv` reads tab-separated text files with the extension `.etsv`.
The first line holds the column names. The reader infers the type of each column: `Int64`, `Float64` or `Utf8`.

Beacon adds these SQL objects for the format:

```sql
SELECT * FROM read_example_tsv(['stations.etsv'], {null_value: '-'});
SELECT * FROM read_example_tsv_schema(['stations.etsv']);
CREATE EXTERNAL TABLE stations STORED AS example_tsv LOCATION 'tsv/' OPTIONS ('null_value' '-');
```

Implement two traits from `beacon_plugin_sdk::file_format`:

| Trait | Methods | Use |
| --- | --- | --- |
| `FormatFactory` | `name`, `extensions`, `output_kind`, `table_function_args`, `create` | Describes the format. `create` gets the table options and returns a `FormatReader`. |
| `FormatReader` | `infer_schema`, `infer_stats`, `open`, `write` | Reads or writes one file in each call. |

Then add the factories to the export macro:

```rust
beacon_plugin_sdk::export_plugin!(scalar = functions, formats = tsv::formats);
```

Beacon does the work for many files. It lists the files and merges their schemas.
It casts each column to the table type, fills a missing column with nulls and adds the partition columns.
It also applies the filters again. So `open` can ignore `ScanRequest::filters` and `ScanRequest::limit`.

Rules for format code:

- The format name and each extension must be free in Beacon. The CSV format of Beacon has the extension `tsv`, so the example uses `etsv`.
- `table_function_args` sets the fields of the options struct of `read_<name>`. Beacon gives each value to `create` as a string.
- Do all I/O through the `ObjectStore` that Beacon gives. Then the format works on local disk and on S3.
- Do not call `tokio::spawn`, `tokio::time` or `Handle::current()`. The plugin has no tokio runtime.
- Keep each poll short. CPU work in a poll blocks one Beacon worker thread.
- `OutputKind::Flat` gives normal Arrow columns. For gridded data, use `OutputKind::Nd` and `beacon_plugin_sdk::nd`.
- Beacon does not call `infer_stats` or `write` yet. So statistics and `COPY ... TO` do not work for a plugin format. The example implements `infer_stats` for a later Beacon version.

The unit tests in `tsv.rs` call the reader with an `InMemory` store.

## Run the unit tests

The unit tests run the native functions in DataFusion and the file format on an in-memory store. They need no Beacon server.

```bash
cargo test --manifest-path native/Cargo.toml
```

## Pack the plugin

Build the library on the platform of the Beacon server.
The Beacon server image is a Linux image. For that image, build on Linux or in WSL.

1. Build the library:

   ```bash
   cargo build --release --manifest-path native/Cargo.toml
   ```

2. Pack the plugin. The script writes the archive and `SHA256SUMS` in `dist/`:

   ```bash
   python3 scripts/package-plugin.py . --out dist --library native/target/release/libexample_native.so --target x86_64-unknown-linux-gnu
   ```

Use the library file and the target of your platform:

| Platform | Library file | Target |
| --- | --- | --- |
| Linux | `libexample_native.so` | `x86_64-unknown-linux-gnu` or `aarch64-unknown-linux-gnu` |
| macOS | `libexample_native.dylib` | `aarch64-apple-darwin` or `x86_64-apple-darwin` |
| Windows | `example_native.dll` | `x86_64-pc-windows-msvc` |

Beacon loads only the library file of its own platform. It picks the archive with its own target.

| Plugin type | Archive name |
| --- | --- |
| native | `<name>-<version>-df<major>-<target>.tar.gz` |
| SQL only | `<name>-<version>-any.tar.gz` |

To try the archive, mount `dist/` into a Beacon container as `/plugins`. Then run:

```sql
INSTALL example FROM 'file:///plugins/example-0.1.0-df54-x86_64-unknown-linux-gnu.tar.gz'
```

To try the file format, put `native/testdata/stations.etsv` in the Beacon data folder. Then run:

```sql
SELECT * FROM read_example_tsv(['stations.etsv'], {null_value: '-'})
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
It builds the library for these targets:

| Runner | Target |
| --- | --- |
| `ubuntu-24.04` | `x86_64-unknown-linux-gnu` |
| `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` |
| `macos-15` | `aarch64-apple-darwin` |
| `macos-15-intel` | `x86_64-apple-darwin` |
| `windows-2025` | `x86_64-pc-windows-msvc` |

Then it attaches the archives and one `SHA256SUMS` file to the release `v0.1.0`.
To skip a target, remove its row from the matrix in `release.yml`.
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

After the install, the functions, `read_example_tsv`, `read_example_tsv_schema` and `STORED AS example_tsv` are available.
`beacon.system.plugins` shows the installed plugins.

`UNINSTALL example` removes the plugin. A table on `example_tsv` then gives an error until you install the plugin again.

## Rules for native code

- A fault in native code stops the Beacon server. Test the plugin well.
- A panic in a function or in a format call becomes a query error.
- Do not panic in getters, such as `name`, `aliases`, `signature` and `extensions`. Do not panic in `Drop`.

## License

Apache-2.0. See [LICENSE](LICENSE).
