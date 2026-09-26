use crate::error::{AppError, Result};
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub instances: PathBuf,
    pub versions: PathBuf,
    pub libraries: PathBuf,
    pub assets: PathBuf,
    pub metadata: PathBuf,
    pub profiles: PathBuf,
    pub settings: PathBuf,
    pub logs: PathBuf,
    pub runtimes: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self> {
        let project = ProjectDirs::from("dev", "Flint", "Flint").ok_or_else(|| {
            AppError::new(
                "app_data_unavailable",
                "The operating system did not provide an application-data directory.",
            )
        })?;
        Ok(Self::at(project.data_dir()))
    }

    pub fn at(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().to_path_buf();
        let minecraft = root.join("minecraft");
        Self {
            instances: root.join("instances"),
            versions: minecraft.join("versions"),
            libraries: minecraft.join("libraries"),
            assets: minecraft.join("assets"),
            metadata: minecraft.join("metadata"),
            profiles: root.join("profiles"),
            settings: root.join("settings"),
            logs: root.join("logs"),
            runtimes: root.join("runtimes"),
        }
    }

    pub fn ensure(&self) -> Result<()> {
        for path in [
            &self.instances,
            &self.versions,
            &self.libraries,
            &self.assets,
            &self.metadata,
            &self.profiles,
            &self.settings,
            &self.logs,
            &self.runtimes,
        ] {
            std::fs::create_dir_all(path)?;
        }
        Ok(())
    }

    pub fn instance_game(&self, id: &str) -> PathBuf {
        self.instances.join(id).join("game")
    }

    pub fn instance(&self, id: &str) -> PathBuf {
        self.instances.join(id)
    }
}
