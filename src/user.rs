use std::fmt::Display;
use std::sync::Arc;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub(crate) struct User {
    pub(crate) name: Arc<String>,
}

impl User {
    pub(crate) fn new(name: &str) -> Self {
        Self {
            name: Arc::new(name.into()),
        }
    }
}

impl Display for User {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}
