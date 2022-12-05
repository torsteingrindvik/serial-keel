use std::fmt::Display;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// A user of the serial keel server.
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    /// The user's name.
    pub name: Arc<String>,
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
