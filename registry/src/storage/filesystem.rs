// Copyright 2023 Helsing GmbH
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use super::*;
use std::path::{PathBuf, Path};
use tokio::fs;

pub struct Filesystem<P: AsRef<Path>> {
    path: P
}

impl<P: AsRef<Path>> Filesystem<P> {
    pub fn new(path: P) -> Self {
        Self { path }
    }

    fn path(&self) -> &Path {
        self.path.as_ref()
    }

    fn package_path(&self, name: &str, version: &str) -> PathBuf {
        self.path().join(format!("{name}-{version}.tar.gz"))
    }

    async fn do_package_put(&self, name: &str, version: &str, data: &[u8]) -> Result<()> {
        let name = self.package_path(name, version);
        fs::write(&name, data).await.into_diagnostic()?;
        Ok(())
    }

    async fn do_package_get(&self, name: &str, version: &str) -> Result<Bytes> {
        let name = self.package_path(name, version);
        fs::read(&name).await.into_diagnostic().map(Into::into)
    }
}

#[async_trait::async_trait]
impl<P: AsRef<Path> + Send + Sync> Storage for Filesystem<P> {
    async fn package_put(&self, package: &str, version: &str, data: &[u8]) -> Result<()> {
        self.do_package_put(package, version, data).await
    }

    async fn package_get(&self, package: &str, version: &str) -> Result<Bytes> {
        self.do_package_get(package, version).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempdir::TempDir;

    #[tokio::test]
    async fn can_write_package() {
        let dir = TempDir::new("storage").unwrap();
        let storage = Filesystem::new(dir.path());

        storage.package_put("mypackage", "0.1.5", &[]).await.unwrap();


    }
}
