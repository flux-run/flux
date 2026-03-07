use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct RequestContext {
    pub user_id: Uuid,
    pub firebase_uid: String,
    pub tenant_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub role: Option<String>,
}
