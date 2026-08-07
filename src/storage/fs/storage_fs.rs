use std::path::{Path, PathBuf};
use tokio::fs;

#[derive(Clone)]
pub struct StorageFs {
    root: PathBuf,
    assets: PathBuf,
}

impl StorageFs {
    pub fn new(root: impl AsRef<Path>, assets: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            assets: assets.as_ref().to_path_buf(),
        }
    }

    pub async fn ensure(&self) -> anyhow::Result<()> {
        fs::create_dir_all(&self.root).await?;
        fs::create_dir_all(&self.assets).await?;
        Ok(())
    }
}
