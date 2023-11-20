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

//! # Buffrs Registry
//!
//! This crate implements a registry for buffrs. The registry is responsible for publishing and
//! making available packages that are published. As such, it also performs authentication and
//! validation on uploaded packages.
//!
//! ## Dependencies
//!
//! It requires two stateful services: one to store metadata, which is expressed by the
//! [`Database`](db::Database) trait. Typically, this would be implemented using a Postgres
//! database, but the code is written in a way that other services can be plugged in instead.
//!
//! The other dependency it has is on a way to store package sources, which is expressed using the
//! [`Storage`](storage::Storage) traits. Typically, this is achieved using an S3 bucket, but other
//! storage mechanisms can be implemented.
//!
//! ## APIs
//!
//! Generally, talking to this registry is possible using a gRPC API, which is defined by the
//! protocol buffer definitions available in this repository and exported as the [`proto`] module.
//!
//! Additionally however, this registry also has some simple REST API endpoints which can be used
//! by simpler clients to access packages. It is not possible to publish packages using these
//! endpoints however.

#![warn(missing_docs)]

pub mod api;
pub mod context;
pub mod proto;
pub mod storage;
pub mod types;
