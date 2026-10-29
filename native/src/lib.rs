//! Example Beacon plugin with four native scalar functions, two of them with planner hooks,
//! and one file format.

mod tsv;

use std::sync::Arc;

use beacon_plugin_sdk::datafusion::arrow::array::{Array, ArrayRef, BooleanArray, Float64Array};
use beacon_plugin_sdk::datafusion::arrow::compute::binary;
use beacon_plugin_sdk::datafusion::arrow::datatypes::DataType;
use beacon_plugin_sdk::datafusion::common::cast::as_float64_array;
use beacon_plugin_sdk::datafusion::common::{Result, ScalarValue, exec_err};
use beacon_plugin_sdk::datafusion::logical_expr::interval_arithmetic::Interval;
use beacon_plugin_sdk::datafusion::logical_expr::{
    ColumnarValue, ScalarFunctionArgs, ScalarUDF, ScalarUDFImpl, Signature, Volatility,
};
use beacon_plugin_sdk::scalar_functions::{Implied, function};

/// The interval that holds only `1.0`.
fn one() -> Result<Interval> {
    let one = ScalarValue::Float64(Some(1.0));
    Interval::try_new(one.clone(), one)
}

/// `example_add_one(x)` returns `x + 1`. It uses the closure helper of the SDK.
///
/// The `bounds` and `propagate` hooks let Beacon turn a range on the result into a range on `x`.
fn add_one() -> ScalarUDF {
    function("example_add_one")
        .args([DataType::Float64])
        .returns(DataType::Float64)
        .alias("example_inc")
        .bounds(|args| Ok(Some(args[0].add(one()?)?)))
        .propagate(|output, args| {
            // An empty intersection means no row matches. No answer is always correct.
            let Some(x) = args[0].intersect(output.sub(one()?)?)? else {
                return Ok(None);
            };
            Ok(Some(vec![x]))
        })
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

/// `example_round(x, digits)` rounds `x` to `digits` decimals. `digits` must be a constant.
///
/// Beacon gives a constant argument as a scalar. So the body computes the factor one time per batch.
fn round_to() -> ScalarUDF {
    function("example_round")
        .args([DataType::Float64, DataType::Int64])
        .returns(DataType::Float64)
        .invoke(|args| {
            let ColumnarValue::Scalar(ScalarValue::Int64(digits)) = &args[1] else {
                return exec_err!("example_round expects a constant digits argument");
            };
            let Some(digits) = digits else {
                return Ok(ColumnarValue::Scalar(ScalarValue::Float64(None)));
            };
            let Ok(digits) = i32::try_from(*digits) else {
                return exec_err!("example_round expects digits in the i32 range");
            };
            let factor = 10f64.powi(digits);
            let round = |v: f64| (v * factor).round() / factor;
            match &args[0] {
                ColumnarValue::Scalar(ScalarValue::Float64(v)) => {
                    Ok(ColumnarValue::Scalar(ScalarValue::Float64(v.map(round))))
                }
                ColumnarValue::Array(array) => {
                    let result: Float64Array = as_float64_array(array)?
                        .iter()
                        .map(|v| v.map(round))
                        .collect();
                    Ok(ColumnarValue::Array(Arc::new(result)))
                }
                other => exec_err!("example_round expects a Float64 argument, got {other:?}"),
            }
        })
}

/// Returns every native function of the plugin.
fn functions() -> Vec<ScalarUDF> {
    vec![
        add_one(),
        ScalarUDF::from(Hypot::new()),
        in_box(),
        round_to(),
    ]
}

beacon_plugin_sdk::export_plugin!(scalar = functions, formats = tsv::formats);

#[cfg(test)]
mod tests {
    use beacon_plugin_sdk::datafusion::common::ScalarValue;
    use beacon_plugin_sdk::datafusion::common::cast::as_float64_array;
    use beacon_plugin_sdk::datafusion::logical_expr::interval_arithmetic::Interval;
    use beacon_plugin_sdk::datafusion::prelude::{SessionContext, col, lit};
    use beacon_plugin_sdk::scalar_functions::{Implied, hooks_of};

    use super::{add_one, functions};

    /// Runs `sql` with the plugin functions and returns the first column as `f64` values.
    async fn run(sql: &str) -> Vec<Option<f64>> {
        let ctx = SessionContext::new();
        for udf in functions() {
            ctx.register_udf(udf);
        }
        let batches = ctx.sql(sql).await.unwrap().collect().await.unwrap();
        batches
            .iter()
            .flat_map(|b| {
                as_float64_array(b.column(0))
                    .unwrap()
                    .iter()
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    #[tokio::test]
    async fn add_one_adds_one() {
        assert_eq!(run("SELECT example_add_one(1.5)").await, [Some(2.5)]);
    }

    #[tokio::test]
    async fn add_one_keeps_null() {
        assert_eq!(
            run("SELECT example_inc(CAST(NULL AS DOUBLE))").await,
            [None]
        );
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

    #[tokio::test]
    async fn round_rounds_a_column() {
        let sql = "SELECT example_round(column1, 1) \
                   FROM (VALUES (1.26), (2.34), (CAST(NULL AS DOUBLE)))";
        assert_eq!(run(sql).await, [Some(1.3), Some(2.3), None]);
    }

    #[tokio::test]
    async fn round_refuses_a_column_for_digits() {
        let ctx = SessionContext::new();
        for udf in functions() {
            ctx.register_udf(udf);
        }
        let sql = "SELECT example_round(column1, column2) FROM (VALUES (1.26, 1))";
        let result = ctx.sql(sql).await.unwrap().collect().await;
        assert!(result.unwrap_err().to_string().contains("constant digits"));
    }

    #[test]
    fn in_box_implies_the_box_filter() {
        let udf = functions()
            .into_iter()
            .find(|f| f.name() == "example_in_box")
            .unwrap();
        let args = [
            col("lon"),
            col("lat"),
            lit(2.0),
            lit(50.0),
            lit(8.0),
            lit(56.0),
        ];
        let ctx = SessionContext::new().task_ctx();
        let implied = hooks_of(&udf).unwrap().implied(&args, &ctx).unwrap();
        let expected = col("lon")
            .between(lit(2.0), lit(8.0))
            .and(col("lat").between(lit(50.0), lit(56.0)));
        assert!(matches!(implied, Some(Implied::Necessary(e)) if e == expected));
    }

    /// The closed interval `[low, high]` of `f64` values.
    fn interval(low: f64, high: f64) -> Interval {
        Interval::try_new(
            ScalarValue::Float64(Some(low)),
            ScalarValue::Float64(Some(high)),
        )
        .unwrap()
    }

    /// Checks that `actual` holds `[low, high]` and is at most a rounding step wider.
    fn assert_near(actual: &Interval, low: f64, high: f64) {
        let (ScalarValue::Float64(Some(a)), ScalarValue::Float64(Some(b))) =
            (actual.lower(), actual.upper())
        else {
            panic!("expected finite Float64 bounds: {actual:?}");
        };
        // Interval arithmetic rounds float bounds outward.
        assert!(
            *a <= low && low - a < 1e-9 && *b >= high && b - high < 1e-9,
            "{actual:?}"
        );
    }

    #[test]
    fn add_one_bounds_shift_the_input() {
        let udf = add_one();
        let bounds = hooks_of(&udf)
            .unwrap()
            .bounds(&[&interval(0.0, 10.0)])
            .unwrap();
        assert_near(&bounds.unwrap(), 1.0, 11.0);
    }

    #[test]
    fn add_one_propagate_gives_the_input_range() {
        let udf = add_one();
        let hooks = hooks_of(&udf).unwrap();
        // example_add_one(x) BETWEEN 5 AND 9, with x in [0, 100], gives x in [4, 8].
        let input = hooks
            .propagate(&interval(5.0, 9.0), &[&interval(0.0, 100.0)])
            .unwrap();
        assert_near(&input.unwrap()[0], 4.0, 8.0);
        // x in [50, 100] cannot give a result in [5, 9].
        let none = hooks
            .propagate(&interval(5.0, 9.0), &[&interval(50.0, 100.0)])
            .unwrap();
        assert_eq!(none, None);
    }
}
