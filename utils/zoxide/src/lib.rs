pub mod cmd;
pub mod config;
pub mod db;
pub mod error;
pub mod util;

#[cfg(test)]
mod test {
    use crate::db::Database;

    #[test]
    fn read_db() -> anyhow::Result<()> {
        let db = Database::open()?;
        db.with_dirs(|dirs| {
            println!("Loaded dirs {dirs:#?}");
            println!("TOTAL [{}]", dirs.len());
        });

        Ok(())
    }
}
