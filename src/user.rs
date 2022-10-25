use std::sync::Arc;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub(crate) struct User {
    name: Arc<String>,
}

impl User {
    pub(crate) fn new(name: &str) -> Self {
        Self {
            name: Arc::new(name.into()),
        }
    }
}
