use std::sync::Arc;

use crate::bundle::cache::BundleCache;
use crate::contracts::{ApiDispatch, DataEngineDispatch, QueueDispatch};
use crate::engine::executor::PoolDispatchers;
use crate::engine::pool::IsolatePool;

#[derive(Clone)]
pub struct AppState {
    pub api: Arc<dyn ApiDispatch>,
    pub queue: Arc<dyn QueueDispatch>,
    pub data_engine: Arc<dyn DataEngineDispatch>,
    pub service_token: String,
    pub bundle_cache: BundleCache,
    pub isolate_pool: IsolatePool,
    pub dispatchers: PoolDispatchers,
}
