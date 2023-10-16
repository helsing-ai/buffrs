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

use proptest::prelude::*;

use std::{ops::Deref, str::FromStr, sync::Arc};
use test_strategy::Arbitrary;

prop_compose! {
    fn package_name()(name in "[a-z][a-z0-9-]{0,127}") -> Arc<str> {
        name.to_string().into()
    }
}

prop_compose! {
    fn package_version()(major: u16, minor: u16, patch: u16) -> Arc<str> {
        format!("{major}.{minor}.{patch}").into()
    }
}

prop_compose! {
    fn token()(token in "[a-z]{64}") -> Arc<str> {
        token.to_string().into()
    }
}

prop_compose! {
    fn handle()(handle in "[a-z]{1,64}") -> Arc<str> {
        handle.to_string().into()
    }
}

// FIXME(xfbs): move this to buffrs
#[derive(Clone, Debug, PartialEq, Eq, Hash, Arbitrary)]
pub struct PackageVersion {
    // FIXME(xfbs): use PackageName here
    /// Package name
    #[strategy(package_name())]
    pub package: Arc<str>,

    // FIXME(xfbs): use Version here
    #[strategy(package_version())]
    /// Package version
    pub version: Arc<str>,
}

impl PackageVersion {
    /// Determine the file name of a package.
    pub fn file_name(&self) -> String {
        let Self { package, version } = &self;
        format!("{package}_{version}.tar.gz")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Arbitrary)]
pub struct Token(#[strategy(token())] Arc<str>);

impl FromStr for Token {
    // FIXME(xfbs): add error
    type Err = ();

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Ok(Token(input.to_string().into()))
    }
}

impl Deref for Token {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Arbitrary)]
pub struct TokenPrefix(#[strategy(token())] Arc<str>);

impl FromStr for TokenPrefix {
    // FIXME(xfbs): add error
    type Err = ();

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Ok(TokenPrefix(input.to_string().into()))
    }
}

impl Deref for TokenPrefix {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Arbitrary)]
pub struct Handle(#[strategy(handle())] Arc<str>);

impl FromStr for Handle {
    // FIXME(xfbs): add error
    type Err = ();

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Ok(Handle(input.to_string().into()))
    }
}

impl Deref for Handle {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
