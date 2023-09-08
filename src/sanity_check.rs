use eyre::ensure;
use url::Url;

pub trait SanityCheck {
    fn sanity_check(&self) -> eyre::Result<()>;
}

impl SanityCheck for Url {
    fn sanity_check(&self) -> eyre::Result<()> {
        tracing::debug!("checking that url begins with http or https: {}", self.scheme());
        ensure!(
            self.scheme() == "http" || self.scheme() == "https",
            "The url must start with http:// or https://"
        );

        tracing::debug!("checking that url ends with /artifactory: {}", self.path());
        ensure!(
            self.path().ends_with("/artifactory"),
            "The url must end with '/artifactory'"
        );

        Ok(())
    }
}