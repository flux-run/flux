use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct RequestContext {
    pub user_id: Uuid,
    pub firebase_uid: String,
    pub tenant_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub tenant_slug: Option<String>,
    pub project_slug: Option<String>,
    pub role: Option<String>,
}
