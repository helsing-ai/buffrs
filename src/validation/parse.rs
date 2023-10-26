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

use std::path::Path;

use crate::validation::data::{Packages, PackagesError};

/// Errors parsing `buffrs` packages.
#[derive(thiserror::Error, Debug)]
#[allow(missing_docs)]
pub enum ParseError {
    #[error(transparent)]
    Parse(#[from] anyhow::Error),
    #[error(transparent)]
    Adding(#[from] PackagesError),
}

/// Parser for `buffrs` packages.
pub struct Parser {
    parser: protobuf_parse::Parser,
}

impl Parser {
    /// Create new parser with a given root path.
    pub fn new(root: &Path) -> Self {
        let mut parser = protobuf_parse::Parser::new();
        parser.pure();
        parser.include(root);

        Self { parser }
    }

    /// Add file to be processed by this parser.
    pub fn input(&mut self, file: &Path) {
        self.parser.input(file);
    }

    /// Parse into [`Packages`].
    pub fn parse(self) -> Result<Packages, ParseError> {
        let fds = self.parser.file_descriptor_set()?;

        let mut packages = Packages::default();

        for file in &fds.file {
            packages.add(file)?;
        }

        Ok(packages)
    }
}
