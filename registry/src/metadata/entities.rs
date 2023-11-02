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

use sqlx::FromRow;

#[derive(Clone, Debug, FromRow)]
pub struct UserInfo {
    pub handle: String,
}

#[derive(Clone, Debug, Default, FromRow)]
pub struct TokenPermissions {
    pub allow_publish: bool,
    pub allow_update: bool,
    pub allow_yank: bool,
}

#[derive(Clone, Debug, FromRow)]
pub struct TokenInfo {
    pub handle: String,
    pub prefix: String,
    pub hash: String,
    pub permissions: TokenPermissions,
}

#[derive(Clone, Debug, FromRow)]
pub struct Package {}
