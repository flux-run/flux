use uuid::Uuid;

pub fn generate_id() -> Uuid {
    Uuid::new_v4()
}