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

use miette::{IntoDiagnostic, ensure};
use reqwest::{Body, Response};
use url::Url;

/// A builder for HTTP requests with authentication support
pub(super) struct RequestBuilder(reqwest::RequestBuilder);

impl RequestBuilder {
    pub(super) fn new(client: reqwest::Client, method: reqwest::Method, url: Url) -> Self {
        Self(client.request(method, url))
    }

    pub(super) fn auth(mut self, token: String) -> Self {
        self.0 = self.0.bearer_auth(token);
        self
    }

    pub(super) fn body(mut self, payload: impl Into<Body>) -> Self {
        self.0 = self.0.body(payload);
        self
    }

    pub(super) async fn send(self) -> miette::Result<ValidatedResponse> {
        self.0.send().await.into_diagnostic()?.try_into()
    }
}

/// A validated HTTP response that ensures no redirects and proper authentication
#[derive(Debug)]
pub(super) struct ValidatedResponse(pub(super) reqwest::Response);

impl TryFrom<Response> for ValidatedResponse {
    type Error = miette::Report;

    fn try_from(value: Response) -> Result<Self, Self::Error> {
        ensure!(
            !value.status().is_redirection(),
            "remote server attempted to redirect request - is this registry URL valid?"
        );

        ensure!(
            value.status() != 401,
            "unauthorized - please provide registry credentials with `buffrs login`"
        );

        value.error_for_status().into_diagnostic().map(Self)
    }
}
