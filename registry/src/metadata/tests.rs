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

pub use proptest::prelude::*;
pub use test_strategy::proptest;

use std::{future::Future, pin::Pin};

/// Generic future used for cleanup tasks.
pub type Cleanup = Pin<Box<dyn Future<Output = ()>>>;

/// Run a closure with a temporary instance and run cleanup afterwards.
pub async fn with<
    S: Metadata,
    O1: Future<Output = (S, Cleanup)>,
    F1: Fn() -> O1,
    O2: Future<Output = ()>,
    F2: FnOnce(S) -> O2,
>(
    function: F1,
    closure: F2,
) {
    let (metadata, cleanup) = function().await;
    closure(metadata).await;
    cleanup.await;
}
