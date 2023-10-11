use super::*;

/// Service definition.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq, Diff)]
#[diff(attr(
    #[derive(Debug)]
    #[allow(missing_docs)]
))]
pub struct Service {}
