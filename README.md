# beacon-plugin-template

An example plugin for [Beacon](https://github.com/maris-development/beacon), and a template to make your own.
The [wiki](https://github.com/maris-development/beacon-plugin-template/wiki) is the full guide.

## What a plugin can do

| A plugin adds | Example in this template | More information |
| --- | --- | --- |
| Native SQL functions, written in Rust | `example_add_one`, `example_hypot`, `example_in_box`, `example_round` | [Scalar functions](https://github.com/maris-development/beacon-plugin-template/wiki/Scalar-Functions) |
| SQL functions, written in SQL | `example_add_two` | [Scalar functions](https://github.com/maris-development/beacon-plugin-template/wiki/Scalar-Functions#sql-functions) |
| Planner hooks, so a filter on a function prunes files | `example_add_one`, `example_in_box` | [Planner hooks](https://github.com/maris-development/beacon-plugin-template/wiki/Planner-Hooks) |
| File formats, so Beacon reads a new type of file | `example_tsv` | [File formats](https://github.com/maris-development/beacon-plugin-template/wiki/File-Formats) |

## Install the plugin

An admin installs the plugin from a GitHub release:

```sql
INSTALL example FROM 'github:<owner>/<repo>@v0.1.0'
```

Beacon downloads the archive for its own platform, checks it against `SHA256SUMS` and loads it.
`BEACON_PLUGINS=example=github:<owner>/<repo>@v0.1.0` installs the plugin at server start.
`SELECT * FROM beacon.system.plugins` shows the installed plugins. `UNINSTALL example` removes the plugin.

More information: [Installing](https://github.com/maris-development/beacon-plugin-template/wiki/Installing).

## Use the plugin

| Function | Result |
| --- | --- |
| `example_add_one(x)`, alias `example_inc(x)` | `x + 1` |
| `example_add_two(x)` | `x + 2` |
| `example_hypot(x, y)` | `sqrt(x² + y²)` |
| `example_in_box(lon, lat, min_lon, min_lat, max_lon, max_lat)` | `true` when the point is in the box |
| `example_round(x, digits)` | `x` rounded to `digits` decimals. `digits` must be a constant. |

```sql
SELECT example_hypot(3.0, 4.0), example_round(3.14159, 2);
```

The file format `example_tsv` reads tab-separated text files with the extension `.etsv`:

```sql
SELECT * FROM read_example_tsv(['stations.etsv'], {null_value: '-'});
CREATE EXTERNAL TABLE stations STORED AS example_tsv LOCATION 'tsv/' OPTIONS ('null_value' '-');
```

## How a plugin works

A plugin is an archive with three parts:

| Part | File | Contents |
| --- | --- | --- |
| Manifest | `beacon-plugin.toml` | The name, the version and the supported Beacon versions. |
| Native library | `native/` | A Rust crate on `beacon-plugin-sdk`. It exports the native functions and the file formats. |
| SQL files | `sql/functions.sql` | `CREATE FUNCTION` statements. |

Beacon loads the library and checks its SDK ABI and DataFusion version.
So build the plugin with the SDK of the Beacon version that runs it.

More information: [Getting started](https://github.com/maris-development/beacon-plugin-template/wiki/Getting-Started).

## Implement a plugin

1. Click **Use this template** on GitHub. Clone the new repository.
2. Rename the plugin in `beacon-plugin.toml`, `native/Cargo.toml` and the code. See [Getting started](https://github.com/maris-development/beacon-plugin-template/wiki/Getting-Started#rename-the-plugin).
3. Write the functions in `native/src/lib.rs`. See [Scalar functions](https://github.com/maris-development/beacon-plugin-template/wiki/Scalar-Functions) and [Planner hooks](https://github.com/maris-development/beacon-plugin-template/wiki/Planner-Hooks).

   ```rust
   function("example_add_one")
       .args([DataType::Float64])
       .returns(DataType::Float64)
       .invoke_arrays(|arrays| { /* compute one value per row */ })
   ```

4. Write the file formats in `native/src/tsv.rs`, or remove it. See [File formats](https://github.com/maris-development/beacon-plugin-template/wiki/File-Formats).

   ```rust
   impl FormatFactory for TsvFormat { /* name, extensions, create */ }
   impl FormatReader for TsvReader { /* infer_schema, open */ }
   ```

5. Export both in `native/src/lib.rs`:

   ```rust
   beacon_plugin_sdk::export_plugin!(scalar = functions, formats = tsv::formats);
   ```

6. Write the SQL functions in `sql/functions.sql`, or remove `[sql]` from the manifest.
7. Run the unit tests. See [Testing](https://github.com/maris-development/beacon-plugin-template/wiki/Testing).

   ```bash
   cargo test --manifest-path native/Cargo.toml
   ```

8. Set `plugin.version` in `beacon-plugin.toml`, commit and push a tag `v<version>`.
   The release workflow builds the archives for Linux, macOS and Windows.
   See [Packaging and releasing](https://github.com/maris-development/beacon-plugin-template/wiki/Packaging-and-Releasing).

An error? See [Troubleshooting](https://github.com/maris-development/beacon-plugin-template/wiki/Troubleshooting).

## License

Apache-2.0. See [LICENSE](LICENSE).
