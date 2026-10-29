# beacon-plugin-template

An example plugin for [Beacon](https://github.com/maris-development/beacon), and a template to make your own.
The plugin `example` adds SQL functions and a file format to Beacon.

## Install the plugin

An admin installs the plugin with SQL. Replace `<owner>/<repo>` and the tag with your repository and release:

```sql
INSTALL example FROM 'github:<owner>/<repo>@v0.1.0'
```

Beacon downloads the archive for its own platform and checks it against `SHA256SUMS`.
To install the plugin at server start, set `BEACON_PLUGINS`:

```bash
BEACON_PLUGINS=example=github:<owner>/<repo>@v0.1.0
```

Check the plugin:

```sql
SELECT name, version, status, error, functions FROM beacon.system.plugins
```

## Use the plugin

| Function | Result |
| --- | --- |
| `example_add_one(x)`, alias `example_inc(x)` | `x + 1` |
| `example_add_two(x)` | `x + 2` |
| `example_hypot(x, y)` | `sqrt(x² + y²)` |
| `example_in_box(lon, lat, min_lon, min_lat, max_lon, max_lat)` | `true` when the point is in the box |
| `example_round(x, digits)` | `x` rounded to `digits` decimals. `digits` must be a constant. |

```sql
SELECT example_round(example_hypot(3.0, 4.0) + 0.123, 1);
```

The file format `example_tsv` reads tab-separated text files with the extension `.etsv`.
The first line holds the column names.

```sql
SELECT * FROM read_example_tsv(['stations.etsv'], {null_value: '-'});
SELECT * FROM read_example_tsv_schema(['stations.etsv']);
CREATE EXTERNAL TABLE stations STORED AS example_tsv LOCATION 'tsv/' OPTIONS ('null_value' '-');
```

`UNINSTALL example` removes the plugin.

## Make your own plugin

1. Click **Use this template** on GitHub. Clone the new repository.
2. Rename the plugin and write your functions and formats.
3. Run the unit tests:

   ```bash
   cargo test --manifest-path native/Cargo.toml
   ```

4. Set `plugin.version` in `beacon-plugin.toml`. Push a tag `v<version>`. The release workflow builds the archives for Linux, macOS and Windows.

The [wiki](https://github.com/maris-development/beacon-plugin-template/wiki) is the full guide:

- [Getting started](https://github.com/maris-development/beacon-plugin-template/wiki/Getting-Started)
- [Scalar functions](https://github.com/maris-development/beacon-plugin-template/wiki/Scalar-Functions)
- [Planner hooks](https://github.com/maris-development/beacon-plugin-template/wiki/Planner-Hooks)
- [File formats](https://github.com/maris-development/beacon-plugin-template/wiki/File-Formats)
- [Testing](https://github.com/maris-development/beacon-plugin-template/wiki/Testing)
- [Packaging and releasing](https://github.com/maris-development/beacon-plugin-template/wiki/Packaging-and-Releasing)
- [Installing](https://github.com/maris-development/beacon-plugin-template/wiki/Installing)
- [Troubleshooting](https://github.com/maris-development/beacon-plugin-template/wiki/Troubleshooting)

## License

Apache-2.0. See [LICENSE](LICENSE).
