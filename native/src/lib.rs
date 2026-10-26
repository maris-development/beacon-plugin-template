//! Example Beacon plugin with three native scalar functions.

use std::sync::Arc;

use beacon_plugin_sdk::datafusion::arrow::array::{Array, ArrayRef, BooleanArray, Float64Array};
use beacon_plugin_sdk::datafusion::arrow::compute::binary;
use beacon_plugin_sdk::datafusion::arrow::datatypes::DataType;
use beacon_plugin_sdk::datafusion::common::cast::as_float64_array;
use beacon_plugin_sdk::datafusion::common::{Result, exec_err};
use beacon_plugin_sdk::datafusion::logical_expr::{
    ColumnarValue, ScalarFunctionArgs, ScalarUDF, ScalarUDFImpl, Signature, Volatility,
};
use beacon_plugin_sdk::scalar_functions::{Implied, function};

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

/// `example_in_box(lon, lat, min_lon, min_lat, max_lon, max_lat)` is true when the point is in the box.
///
/// The `implies` hook lets Beacon prune files with the `lon` and `lat` statistics.
fn in_box() -> ScalarUDF {
    function("example_in_box")
        .args(vec![DataType::Float64; 6])
        .returns(DataType::Boolean)
        .implies(|args| {
            let lon = args[0].clone().between(args[2].clone(), args[4].clone());
            let lat = args[1].clone().between(args[3].clone(), args[5].clone());
            Ok(Some(Implied::Necessary(lon.and(lat))))
        })
        .invoke_arrays(|arrays| {
            let columns = arrays
                .iter()
                .map(|a| as_float64_array(a))
                .collect::<Result<Vec<_>>>()?;
            let result: BooleanArray = (0..arrays[0].len())
                .map(|row| {
                    // A null argument gives a null result.
                    if columns.iter().any(|c| c.is_null(row)) {
                        return None;
                    }
                    let v = |i: usize| columns[i].value(row);
                    Some(v(0) >= v(2) && v(0) <= v(4) && v(1) >= v(3) && v(1) <= v(5))
                })
                .collect();
            Ok(Arc::new(result) as ArrayRef)
        })
}

/// Returns every native function of the plugin.
fn functions() -> Vec<ScalarUDF> {
    vec![add_one(), ScalarUDF::from(Hypot::new()), in_box()]
}

beacon_plugin_sdk::export_plugin!(scalar = functions);

#[cfg(test)]
mod tests {
    use beacon_plugin_sdk::datafusion::common::cast::as_float64_array;
    use beacon_plugin_sdk::datafusion::prelude::{SessionContext, col, lit};
    use beacon_plugin_sdk::scalar_functions::{Implied, hooks_of};

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

    #[tokio::test]
    async fn in_box_checks_the_point() {
        let sql = "SELECT CAST(example_in_box(column1, column2, 2.0, 50.0, 8.0, 56.0) AS DOUBLE) \
                   FROM (VALUES (5.0, 52.0), (1.0, 52.0), (CAST(NULL AS DOUBLE), 52.0))";
        assert_eq!(run(sql).await, [Some(1.0), Some(0.0), None]);
    }

    #[test]
    fn in_box_implies_the_box_filter() {
        let udf = functions().into_iter().find(|f| f.name() == "example_in_box").unwrap();
        let args = [col("lon"), col("lat"), lit(2.0), lit(50.0), lit(8.0), lit(56.0)];
        let implied = hooks_of(&udf).unwrap().implied(&args).unwrap();
        let expected = col("lon")
            .between(lit(2.0), lit(8.0))
            .and(col("lat").between(lit(50.0), lit(56.0)));
        assert!(matches!(implied, Some(Implied::Necessary(e)) if e == expected));
    }
}
