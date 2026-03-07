use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub firebase_uid: String,
    pub email: String,
    pub created_at: chrono::NaiveDateTime,
}
