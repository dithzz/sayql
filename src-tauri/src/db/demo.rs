use crate::error::AppResult;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

/// Creates the bundled demo database if it does not exist yet.
pub async fn ensure_demo(path: &std::path::Path) -> AppResult<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let o = SqliteConnectOptions::new().filename(path).create_if_missing(true);
    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(o).await?;
    let seed = include_str!("demo.sql");
    if let Err(e) = sqlx::raw_sql(seed).execute(&pool).await {
        pool.close().await;
        let _ = std::fs::remove_file(path);
        return Err(format!("Could not create demo database: {e}").into());
    }
    pool.close().await;
    Ok(())
}
