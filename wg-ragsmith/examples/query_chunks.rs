use once_cell::sync::OnceCell;
use std::mem::transmute;
use std::os::raw::c_char;
use tokio_rusqlite::{ffi, Connection, Result};

#[tokio::main]
async fn main() -> Result<()> {
    // Register sqlite-vec extension
    register_sqlite_vec().unwrap();

    let conn = Connection::open("rust_book_chunks.sqlite").await?;

    // Verify sqlite-vec is working
    let version = conn
        .call(|conn| {
            match conn.query_row("select vec_version()", [], |row| row.get::<_, String>(0)) {
                Ok(v) => Ok(v),
                Err(e) => Err(tokio_rusqlite::Error::Rusqlite(e)),
            }
        })
        .await;

    match version {
        Ok(v) => println!("sqlite-vec version: {}", v),
        Err(e) => {
            println!("Failed to get sqlite-vec version: {}", e);
            return Ok(());
        }
    }

    // Let's try a simpler query first - just get chunks without embeddings
    let results = conn
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT c.id,
                    substr(replace(c.content, char(10), ' '), 1, 80) AS preview
             FROM chunks AS c
             LIMIT 5",
            )?;

            let chunk_iter = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?, // id
                    row.get::<_, String>(1)?, // preview
                ))
            })?;

            let mut chunks = Vec::new();
            for chunk in chunk_iter {
                chunks.push(chunk?);
            }
            Ok(chunks)
        })
        .await?;

    println!("ID                                    | Preview");
    println!(
        "--------------------------------------|--------------------------------------------------"
    );

    for (id, preview) in results {
        println!("{:<37} | {}", id, preview);
    }

    // Try to get count from embeddings table (this will fail without extension)
    let embedding_result = conn
        .call(move |conn| {
            match conn.query_row("SELECT COUNT(*) FROM chunks_embeddings", [], |row| {
                row.get::<_, i64>(0)
            }) {
                Ok(count) => Ok(count),
                Err(e) => Err(tokio_rusqlite::Error::Rusqlite(e)),
            }
        })
        .await;

    match embedding_result {
        Ok(count) => {
            println!("\nTotal embeddings: {}", count);
        }
        Err(e) => {
            println!("\nCould not access embeddings table: {}", e);
        }
    }

    // Now let's try the original query with vec_to_json
    let embedding_query_result = conn
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT c.id,
                    substr(replace(c.content, char(10), ' '), 1, 80) AS preview,
                    substr(vec_to_json(e.embedding), 1, 80) || ' …' AS embedding_preview
             FROM   chunks            AS c
             JOIN   chunks_embeddings AS e
                    ON e.rowid = c.rowid
             LIMIT 5",
            )?;

            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?, // id
                    row.get::<_, String>(1)?, // preview
                    row.get::<_, String>(2)?, // embedding_preview
                ))
            })?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row?);
            }
            Ok(results)
        })
        .await;

    match embedding_query_result {
        Ok(results) => {
            println!("\n=== Chunks with Embeddings ===");
            println!("{:<37} | {:<48} | Embedding Preview", "ID", "Preview");
            println!("{:-<37}-|-{:-<48}-|{:-<50}", "", "", "");
            for (id, preview, embedding_preview) in results {
                println!("{:<37} | {:<48} | {}", id, preview, embedding_preview);
            }
        }
        Err(e) => {
            println!("\nFailed to query embeddings with vec_to_json: {}", e);
        }
    }

    Ok(())
}

fn register_sqlite_vec() -> std::result::Result<(), String> {
    static REGISTER: OnceCell<()> = OnceCell::new();
    REGISTER
        .get_or_try_init(|| {
            unsafe {
                type SqliteExtensionInit = unsafe extern "C" fn(
                    *mut ffi::sqlite3,
                    *mut *mut c_char,
                    *const ffi::sqlite3_api_routines,
                ) -> i32;

                let init: unsafe extern "C" fn() = sqlite_vec::sqlite3_vec_init;
                let init_fn: SqliteExtensionInit =
                    transmute::<unsafe extern "C" fn(), SqliteExtensionInit>(init);
                let rc = ffi::sqlite3_auto_extension(Some(init_fn));
                if rc != 0 {
                    return Err(format!(
                        "failed to register sqlite-vec extension (code {})",
                        rc
                    ));
                }
            }
            Ok(())
        })
        .map(|_| ())
        .map_err(|e| e.clone())
}
