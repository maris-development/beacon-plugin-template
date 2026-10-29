//! `example_tsv`: a file format for tab-separated text files.
//!
//! The first line holds the column names. Each next line holds one row.
//! The reader infers the type of each column: `Int64`, `Float64` or `Utf8`.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::Arc;

use async_trait::async_trait;
use beacon_plugin_sdk::datafusion::arrow::array::{
    ArrayRef, RecordBatch, RecordBatchOptions, StringArray,
};
use beacon_plugin_sdk::datafusion::arrow::compute::cast;
use beacon_plugin_sdk::datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use beacon_plugin_sdk::datafusion::common::stats::Precision;
use beacon_plugin_sdk::datafusion::common::{Statistics, exec_err};
use beacon_plugin_sdk::datafusion::error::{DataFusionError, Result};
use beacon_plugin_sdk::datafusion::execution::SendableRecordBatchStream;
use beacon_plugin_sdk::datafusion::physical_plan::stream::RecordBatchStreamAdapter;
use beacon_plugin_sdk::file_format::{
    ArgKind, ArgSpec, FormatFactory, FormatReader, OutputKind, ScanRequest,
};
use beacon_plugin_sdk::object_store::store::{ObjectMeta, ObjectStore, ObjectStoreExt};
use futures::stream;

/// The option key for the text that means a null value. The default is the empty text.
const NULL_VALUE: &str = "null_value";

/// The `example_tsv` format.
///
/// Beacon adds `STORED AS example_tsv`, `read_example_tsv` and `read_example_tsv_schema`.
#[derive(Debug)]
pub struct TsvFormat;

impl FormatFactory for TsvFormat {
    fn name(&self) -> &str {
        "example_tsv"
    }

    fn extensions(&self) -> Vec<String> {
        // The CSV format of Beacon has the key `tsv`. Each key must be free.
        vec!["etsv".to_string()]
    }

    fn output_kind(&self) -> OutputKind {
        OutputKind::Flat
    }

    /// `read_example_tsv(paths, {null_value: '-'})` sets the `null_value` option.
    fn table_function_args(&self) -> Vec<ArgSpec> {
        vec![ArgSpec {
            option_key: NULL_VALUE.to_string(),
            kind: ArgKind::String,
        }]
    }

    fn create(&self, options: &HashMap<String, String>) -> Result<Arc<dyn FormatReader>> {
        let null_value = options.get(NULL_VALUE).cloned().unwrap_or_default();
        Ok(Arc::new(TsvReader { null_value }))
    }
}

/// The `example_tsv` format with its table options.
#[derive(Debug)]
struct TsvReader {
    null_value: String,
}

/// The column names and the rows of one file. `None` is a null value.
struct Table {
    names: Vec<String>,
    rows: Vec<Vec<Option<String>>>,
}

impl Table {
    /// The values of column `index`, without the null values.
    fn values(&self, index: usize) -> impl Iterator<Item = &str> + Clone {
        self.rows
            .iter()
            .filter_map(move |row| row[index].as_deref())
    }
}

impl TsvReader {
    /// Reads and parses one file. All I/O goes through the store that Beacon gives.
    async fn read(&self, store: &Arc<dyn ObjectStore>, object: &ObjectMeta) -> Result<Table> {
        let bytes = store
            .get(&object.location)
            .await
            .map_err(external)?
            .bytes()
            .await
            .map_err(external)?;
        let text = std::str::from_utf8(&bytes).map_err(external)?;
        self.parse(text, &object.location)
    }

    fn parse(&self, text: &str, location: impl Display) -> Result<Table> {
        let mut lines = text.lines();
        let Some(header) = lines.next() else {
            return exec_err!("{location}: the file has no header line");
        };
        let names: Vec<String> = header.split('\t').map(str::to_string).collect();
        let mut rows = Vec::new();
        for (number, line) in (2..).zip(lines) {
            if line.is_empty() {
                continue;
            }
            let row: Vec<Option<String>> = line
                .split('\t')
                .map(|value| (value != self.null_value).then(|| value.to_string()))
                .collect();
            if row.len() != names.len() {
                return exec_err!(
                    "{location}: line {number} has {} values, the header has {}",
                    row.len(),
                    names.len()
                );
            }
            rows.push(row);
        }
        Ok(Table { names, rows })
    }
}

/// The narrowest type that holds all `values`: `Int64`, `Float64` or `Utf8`.
fn infer_type<'a>(values: impl Iterator<Item = &'a str> + Clone) -> DataType {
    if values.clone().next().is_none() {
        DataType::Utf8
    } else if values.clone().all(|v| v.parse::<i64>().is_ok()) {
        DataType::Int64
    } else if values.clone().all(|v| v.parse::<f64>().is_ok()) {
        DataType::Float64
    } else {
        DataType::Utf8
    }
}

fn external(error: impl std::error::Error + Send + Sync + 'static) -> DataFusionError {
    DataFusionError::External(Box::new(error))
}

#[async_trait]
impl FormatReader for TsvReader {
    async fn infer_schema(
        &self,
        store: Arc<dyn ObjectStore>,
        object: &ObjectMeta,
    ) -> Result<SchemaRef> {
        let table = self.read(&store, object).await?;
        let fields: Vec<Field> = table
            .names
            .iter()
            .enumerate()
            .map(|(index, name)| Field::new(name, infer_type(table.values(index)), true))
            .collect();
        Ok(Arc::new(Schema::new(fields)))
    }

    /// The row count. The columns of `table_schema` get unknown statistics.
    ///
    /// Beacon does not call this method yet.
    async fn infer_stats(
        &self,
        store: Arc<dyn ObjectStore>,
        object: &ObjectMeta,
        table_schema: SchemaRef,
    ) -> Result<Option<Statistics>> {
        let table = self.read(&store, object).await?;
        let mut statistics = Statistics::new_unknown(&table_schema);
        statistics.num_rows = Precision::Exact(table.rows.len());
        Ok(Some(statistics))
    }

    async fn open(
        &self,
        store: Arc<dyn ObjectStore>,
        object: &ObjectMeta,
        request: ScanRequest,
    ) -> Result<SendableRecordBatchStream> {
        let table = self.read(&store, object).await?;
        let mut fields = Vec::new();
        let mut columns = Vec::new();
        for field in request.schema.fields() {
            // Beacon fills a column that this file does not have with nulls.
            let Some(index) = table.names.iter().position(|name| name == field.name()) else {
                continue;
            };
            let text: StringArray = table.rows.iter().map(|row| row[index].as_deref()).collect();
            // `request.schema` holds the merged table type. A value that does not fit becomes null.
            columns.push(cast(&(Arc::new(text) as ArrayRef), field.data_type())?);
            fields.push(Field::new(field.name(), field.data_type().clone(), true));
        }
        let schema = Arc::new(Schema::new(fields));
        // `count(*)` asks for no columns, so set the row count.
        let options = RecordBatchOptions::new().with_row_count(Some(table.rows.len()));
        let batch = RecordBatch::try_new_with_options(Arc::clone(&schema), columns, &options)?;
        let size = request.batch_size.max(1);
        let batches: Vec<Result<RecordBatch>> = (0..batch.num_rows())
            .step_by(size)
            .map(|offset| Ok(batch.slice(offset, size.min(batch.num_rows() - offset))))
            .collect();
        Ok(Box::pin(RecordBatchStreamAdapter::new(
            schema,
            stream::iter(batches),
        )))
    }
}

/// Returns every file format of the plugin.
pub fn formats() -> Vec<Arc<dyn FormatFactory>> {
    vec![Arc::new(TsvFormat)]
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use beacon_plugin_sdk::datafusion::arrow::array::{Array, Int64Array, StringArray};
    use beacon_plugin_sdk::datafusion::arrow::datatypes::{DataType, Field, Schema};
    use beacon_plugin_sdk::datafusion::common::stats::Precision;
    use beacon_plugin_sdk::file_format::{FormatFactory, FormatReader, ScanRequest};
    use beacon_plugin_sdk::object_store::store::memory::InMemory;
    use beacon_plugin_sdk::object_store::store::path::Path;
    use beacon_plugin_sdk::object_store::store::{ObjectMeta, ObjectStore, ObjectStoreExt};
    use futures::TryStreamExt;

    use super::{NULL_VALUE, TsvFormat, TsvReader};

    const STATIONS: &str = include_str!("../testdata/stations.etsv");

    /// Puts the sample file in an in-memory store.
    async fn stored() -> (Arc<dyn ObjectStore>, ObjectMeta) {
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let path = Path::from("stations.etsv");
        store
            .put(&path, STATIONS.as_bytes().to_vec().into())
            .await
            .unwrap();
        let object = store.head(&path).await.unwrap();
        (store, object)
    }

    fn reader(null_value: Option<&str>) -> Arc<dyn FormatReader> {
        let options = null_value
            .map(|v| (NULL_VALUE.to_string(), v.to_string()))
            .into_iter()
            .collect();
        TsvFormat.create(&options).unwrap()
    }

    async fn types(null_value: Option<&str>) -> Vec<(String, DataType)> {
        let (store, object) = stored().await;
        let schema = reader(null_value)
            .infer_schema(store, &object)
            .await
            .unwrap();
        schema
            .fields()
            .iter()
            .map(|f| (f.name().clone(), f.data_type().clone()))
            .collect()
    }

    #[tokio::test]
    async fn infer_schema_finds_the_types() {
        let expected = [
            ("station", DataType::Utf8),
            ("lon", DataType::Float64),
            ("lat", DataType::Float64),
            ("depth", DataType::Int64),
        ]
        .map(|(name, t)| (name.to_string(), t));
        assert_eq!(types(Some("-")).await, expected);
    }

    #[tokio::test]
    async fn without_null_value_a_dash_is_text() {
        assert_eq!(types(None).await[3], ("depth".to_string(), DataType::Utf8));
    }

    #[tokio::test]
    async fn infer_stats_counts_the_rows() {
        let (store, object) = stored().await;
        let schema = Arc::new(Schema::empty());
        let statistics = reader(Some("-"))
            .infer_stats(store, &object, schema)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(statistics.num_rows, Precision::Exact(3));
    }

    #[tokio::test]
    async fn open_gives_the_wanted_columns_in_batches() {
        let (store, object) = stored().await;
        let schema = Arc::new(Schema::new(vec![
            Field::new("depth", DataType::Int64, true),
            Field::new("station", DataType::Utf8, true),
            Field::new("missing", DataType::Float64, true),
        ]));
        let request = ScanRequest {
            schema,
            filters: vec![],
            limit: None,
            batch_size: 2,
        };
        let stream = reader(Some("-"))
            .open(store, &object, request)
            .await
            .unwrap();
        let batches: Vec<_> = stream.try_collect().await.unwrap();
        assert_eq!(
            batches.iter().map(|b| b.num_rows()).collect::<Vec<_>>(),
            [2, 1]
        );
        let mut depth = Vec::new();
        let mut station = Vec::new();
        for batch in &batches {
            // The file has no column `missing`, so the batch leaves it out.
            assert_eq!(batch.num_columns(), 2);
            let d = batch
                .column(0)
                .as_any()
                .downcast_ref::<Int64Array>()
                .unwrap();
            let s = batch
                .column(1)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            depth.extend(d.iter());
            station.extend(s.iter().map(|v| v.map(str::to_string)));
        }
        assert_eq!(depth, [Some(40), None, Some(8)]);
        assert_eq!(station[2].as_deref(), Some("Wadden"));
    }

    #[test]
    fn a_short_row_is_an_error() {
        let reader = TsvReader {
            null_value: String::new(),
        };
        let Err(error) = reader.parse("a\tb\n1\n", "short.etsv") else {
            panic!("expected an error");
        };
        assert!(error.to_string().contains("line 2 has 1 values"), "{error}");
    }
}
