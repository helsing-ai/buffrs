use super::*;

/// Entity that can be defined in a protocol buffer file.
#[derive(Serialize, Deserialize, Clone, Debug, derive_more::From, PartialEq, Eq, Diff)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[diff(attr(
    #[derive(Debug)]
    #[allow(missing_docs)]
))]
pub enum Entity {
    /// Enumeration.
    Enum(Enum),
    /// Service definition.
    Service(Service),
    /// Message definition.
    Message(Message),
}

impl Entity {
    /// Check [`Entity`] against [`RuleSet`] for [`Violations`].
    pub fn check(&self, _rules: &mut RuleSet) -> Violations {
        Violations::default()
    }
}
