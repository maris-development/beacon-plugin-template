//! `example_memory`: a table type that keeps its rows in memory.
//!
//! `CREATE EXTERNAL TABLE notes (id BIGINT, note VARCHAR) STORED AS example_memory LOCATION 'memory'`
//! makes an empty table. `INSERT INTO` adds rows, and `SELECT` reads them.
//! Beacon calls `create` again at each start, so the rows go away when the server stops.

use std::sync::Arc;

use async_trait::async_trait;
use beacon_plugin_sdk::datafusion::catalog::{Session, TableProvider, TableProviderFactory};
use beacon_plugin_sdk::datafusion::common::{Result, plan_err};
use beacon_plugin_sdk::datafusion::datasource::MemTable;
use beacon_plugin_sdk::datafusion::logical_expr::CreateExternalTable;
use beacon_plugin_sdk::table_type::{TableType, table_option};

/// The option key for the number of memory partitions. More partitions let a scan use more threads.
const PARTITIONS: &str = "partitions";

/// Makes the table of each `CREATE EXTERNAL TABLE ... STORED AS example_memory`.
#[derive(Debug)]
struct MemoryFactory;

#[async_trait]
impl TableProviderFactory for MemoryFactory {
    async fn create(
        &self,
        _state: &dyn Session,
        cmd: &CreateExternalTable,
    ) -> Result<Arc<dyn TableProvider>> {
        if cmd.schema.fields().is_empty() {
            return plan_err!("example_memory tables need a column list");
        }
        let partitions = match table_option(cmd, PARTITIONS) {
            None => 1,
            Some(text) => match text.parse::<usize>() {
                Ok(count) if (1..=64).contains(&count) => count,
                _ => {
                    return plan_err!("example_memory: {PARTITIONS} must be 1 to 64, not `{text}`");
                }
            },
        };
        // `MemTable` implements `scan` and `insert_into`, so SELECT and INSERT INTO work.
        let table = MemTable::try_new(Arc::clone(cmd.schema.inner()), vec![vec![]; partitions])?;
        Ok(Arc::new(table))
    }
}

/// Returns every table type of the plugin.
pub fn tables() -> Vec<TableType> {
    vec![TableType::new("example_memory", Arc::new(MemoryFactory))]
}

#[cfg(test)]
mod tests {
    use beacon_plugin_sdk::datafusion::arrow::array::Int64Array;
    use beacon_plugin_sdk::datafusion::prelude::SessionContext;

    use super::tables;

    /// A DataFusion session with the table types under their `STORED AS` names.
    fn session() -> SessionContext {
        let ctx = SessionContext::new();
        let state = ctx.state_ref();
        let mut state = state.write();
        for table_type in tables() {
            let name = table_type.name.to_ascii_uppercase();
            state.table_factories_mut().insert(name, table_type.factory);
        }
        drop(state);
        ctx
    }

    async fn sum_of_id(ctx: &SessionContext) -> i64 {
        let batches = ctx
            .sql("SELECT sum(id) FROM notes")
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();
        let sum = batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        sum.value(0)
    }

    #[tokio::test]
    async fn insert_and_select() {
        let ctx = session();
        ctx.sql(
            "CREATE EXTERNAL TABLE notes (id BIGINT, note VARCHAR) \
             STORED AS example_memory LOCATION 'memory' OPTIONS ('partitions' '2')",
        )
        .await
        .unwrap();
        let insert = "INSERT INTO notes VALUES (1, 'a'), (2, 'b'), (3, 'c')";
        ctx.sql(insert).await.unwrap().collect().await.unwrap();
        assert_eq!(sum_of_id(&ctx).await, 6);
    }

    #[tokio::test]
    async fn a_table_needs_a_column_list() {
        let ctx = session();
        let sql = "CREATE EXTERNAL TABLE notes STORED AS example_memory LOCATION 'memory'";
        let error = ctx.sql(sql).await.unwrap_err().to_string();
        assert!(error.contains("need a column list"), "{error}");
    }

    #[tokio::test]
    async fn partitions_must_be_in_range() {
        let ctx = session();
        let sql = "CREATE EXTERNAL TABLE notes (id BIGINT) STORED AS example_memory \
                   LOCATION 'memory' OPTIONS ('partitions' '0')";
        let error = ctx.sql(sql).await.unwrap_err().to_string();
        assert!(error.contains("must be 1 to 64"), "{error}");
    }
}
