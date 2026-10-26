//! Example Beacon plugin with two native scalar functions.

use std::sync::Arc;

use beacon_plugin_sdk::datafusion::arrow::array::{Array, Float64Array};
use beacon_plugin_sdk::datafusion::arrow::compute::binary;
use beacon_plugin_sdk::datafusion::arrow::datatypes::DataType;
use beacon_plugin_sdk::datafusion::common::cast::as_float64_array;
use beacon_plugin_sdk::datafusion::common::{Result, exec_err};
use beacon_plugin_sdk::datafusion::logical_expr::{
    ColumnarValue, ScalarFunctionArgs, ScalarUDF, ScalarUDFImpl, Signature, Volatility,
};
use beacon_plugin_sdk::function;

/// `example_add_one(x)` returns `x + 1`. It uses the closure helper of the SDK.
fn add_one() -> ScalarUDF {
    function("example_add_one")
        .args([DataType::Float64])
        .returns(DataType::Float64)
        .alias("example_inc")
        .invoke_arrays(|arrays| {
            let Some(values) = arrays[0].as_any().downcast_ref::<Float64Array>() else {
                return exec_err!("example_add_one expects a Float64 argument");
            };
            let result: Float64Array = values.iter().map(|v| v.map(|v| v + 1.0)).collect();
            Ok(Arc::new(result))
        })
}

/// `example_hypot(x, y)` returns `sqrt(x² + y²)`. It implements `ScalarUDFImpl` for full control.
#[derive(Debug, PartialEq, Eq, Hash)]
struct Hypot {
    signature: Signature,
}

impl Hypot {
    fn new() -> Self {
        Self {
            signature: Signature::exact(
                vec![DataType::Float64, DataType::Float64],
                Volatility::Immutable,
            ),
        }
    }
}

impl ScalarUDFImpl for Hypot {
    fn name(&self) -> &str {
        "example_hypot"
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn return_type(&self, _arg_types: &[DataType]) -> Result<DataType> {
        Ok(DataType::Float64)
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        let x = args.args[0].to_array(args.number_rows)?;
        let y = args.args[1].to_array(args.number_rows)?;
        let result: Float64Array = binary(
            as_float64_array(&x)?,
            as_float64_array(&y)?,
            |x: f64, y: f64| x.hypot(y),
        )?;
        Ok(ColumnarValue::Array(Arc::new(result)))
    }
}

/// Returns every native function of the plugin.
fn functions() -> Vec<ScalarUDF> {
    vec![add_one(), ScalarUDF::from(Hypot::new())]
}

beacon_plugin_sdk::export_plugin!(functions);

#[cfg(test)]
mod tests {
    use beacon_plugin_sdk::datafusion::common::cast::as_float64_array;
    use beacon_plugin_sdk::datafusion::prelude::SessionContext;

    use super::functions;

    /// Runs `sql` with the plugin functions and returns the first column as `f64` values.
    async fn run(sql: &str) -> Vec<Option<f64>> {
        let ctx = SessionContext::new();
        for udf in functions() {
            ctx.register_udf(udf);
        }
        let batches = ctx.sql(sql).await.unwrap().collect().await.unwrap();
        batches
            .iter()
            .flat_map(|b| as_float64_array(b.column(0)).unwrap().iter().collect::<Vec<_>>())
            .collect()
    }

    #[tokio::test]
    async fn add_one_adds_one() {
        assert_eq!(run("SELECT example_add_one(1.5)").await, [Some(2.5)]);
    }

    #[tokio::test]
    async fn add_one_keeps_null() {
        assert_eq!(run("SELECT example_inc(CAST(NULL AS DOUBLE))").await, [None]);
    }

    #[tokio::test]
    async fn add_one_works_on_a_column() {
        let sql = "SELECT example_add_one(column1) FROM (VALUES (1.0), (2.0))";
        assert_eq!(run(sql).await, [Some(2.0), Some(3.0)]);
    }

    #[tokio::test]
    async fn hypot_mixes_columns_and_scalars() {
        let sql = "SELECT example_hypot(column1, 4.0) FROM (VALUES (3.0), (0.0))";
        assert_eq!(run(sql).await, [Some(5.0), Some(4.0)]);
    }
}
