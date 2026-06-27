use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompatibilityTarget {
    pub name: String,
    pub status: String,
}
