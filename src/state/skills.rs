//! Atomic full managed-skill receipt, in the existing workspace state store.
use super::Db;
use crate::skills::install_transaction::InstallReceipt;
use anyhow::Result;
use rusqlite::OptionalExtension as _;

impl Db {
    pub(crate) fn invalidate_skills_receipt(&self) -> Result<()> {
        self.conn.execute(
            "DELETE FROM meta WHERE key IN ('skills_install_receipt', 'skills_synced_version')",
            [],
        )?;
        Ok(())
    }

    pub(crate) fn skills_install_receipt(&self) -> Result<Option<InstallReceipt>> {
        let value: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'skills_install_receipt'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        value
            .map(|value| serde_json::from_str(&value).map_err(Into::into))
            .transpose()
    }

    pub(crate) fn set_skills_install_receipt(&self, receipt: &InstallReceipt) -> Result<()> {
        let transaction = self.conn.unchecked_transaction()?;
        for (key, value) in [
            ("skills_install_receipt", serde_json::to_string(receipt)?),
            (
                "skills_synced_version",
                receipt.version.binary_version.clone(),
            ),
        ] {
            transaction.execute("INSERT INTO meta (key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [key, &value])?;
        }
        transaction.commit()?;
        Ok(())
    }
}
