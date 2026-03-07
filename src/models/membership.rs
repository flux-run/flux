use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct Membership {
    pub tenant_id: Uuid,
    pub user_id: Uuid,
    pub role: String,
    pub joined_at: chrono::NaiveDateTime,
}
