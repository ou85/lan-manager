use crate::model::{Snapshot, validate};
use anyhow::{Context, Result, bail};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
const STATE: TableDefinition<&str, &[u8]> = TableDefinition::new("state");

pub struct Store {
    db: Database,
}
impl Store {
    pub fn create(path: &Path, snapshot: &Snapshot) -> Result<Self> {
        // create_new prevents accidental replacement of an existing database.
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        options.open(path)?;
        let store = Self {
            db: Database::create(path)?,
        };
        store.replace(snapshot)?;
        Ok(store)
    }
    pub fn open(path: &Path) -> Result<Self> {
        if !path.is_file() {
            bail!("Database not found. Run 'homelab init' first.");
        }
        let store=Self { db: Database::open(path).context("Cannot open database. Stop the running server before using init, passwd, backup, or restore.")? };
        store.read()?;
        Ok(store)
    }
    pub fn read(&self) -> Result<Snapshot> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(STATE)?;
        let value = table
            .get("inventory")?
            .context("Database is not initialized")?;
        let snapshot: Snapshot = serde_json::from_slice(value.value())?;
        validate(&snapshot)?;
        Ok(snapshot)
    }
    pub fn replace(&self, snapshot: &Snapshot) -> Result<()> {
        validate(snapshot)?;
        let tx = self.db.begin_write()?;
        {
            let mut t = tx.open_table(STATE)?;
            let bytes = serde_json::to_vec(snapshot)?;
            t.insert("inventory", bytes.as_slice())?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn update(&self, f: impl FnOnce(&mut Snapshot) -> Result<()>) -> Result<Snapshot> {
        let tx = self.db.begin_write()?;
        let mut snapshot;
        {
            let mut t = tx.open_table(STATE)?;
            snapshot = {
                let value = t.get("inventory")?.context("Database is not initialized")?;
                serde_json::from_slice::<Snapshot>(value.value())?
            };
            f(&mut snapshot)?;
            validate(&snapshot)?;
            let bytes = serde_json::to_vec(&snapshot)?;
            t.insert("inventory", bytes.as_slice())?;
        }
        tx.commit()?;
        Ok(snapshot)
    }
}
