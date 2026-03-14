//! TypeScript type export tests.
//!
//! Run with:  cargo test -p api_contract --features ts
//! Output:    shared/api_contract/bindings/
//! Then copy: make types   (copies bindings/ → packages/api-types/src/bindings/)

#[cfg(all(test, feature = "ts"))]
mod ts_export {
    use ts_rs::TS;

    macro_rules! export_all {
        ($($ty:ty),+ $(,)?) => {
            $( <$ty>::export_all().expect(concat!("failed to export ", stringify!($ty))); )+
        };
    }

    #[test]
    fn export_typescript_bindings() {
        use crate::{
            api_keys::{ApiKeyRow, CreateApiKeyPayload, CreateApiKeyResponse},
            db_migrate::{MigrateRequest, MigrateResponse},
            deployments::{
                CreateDeploymentPayload, CreateProjectDeploymentPayload,
                DeploySummary, DeploymentResponse, FunctionDeploySummaryEntry,
            },
            environments::{CloneEnvPayload, CreateEnvPayload, EnvironmentRow},
            events::{
                CreateSubscriptionPayload, EventRow, EventSubscriptionRow,
                PublishEventPayload,
            },
            functions::{
                CreateFunctionPayload, CreateFunctionResponse, FunctionResponse,
            },
            gateway::{
                CorsPayload, CreateRoutePayload, MiddlewareCreatePayload,
                RateLimitPayload, RouteFullRow, RoutePayloadEntry, RouteRow,
                SyncRoutesPayload, UpdateRoutePayload,
            },
            queue::{
                CreateQueuePayload, DeadLetterJobRow, PublishMessagePayload,
                QueueConfigRow,
            },
            schedules::{CreateSchedulePayload, CronJobRow},
            secrets::{CreateSecretRequest, SecretResponse, UpdateSecretRequest},
        };

        export_all!(
            // api_keys
            ApiKeyRow, CreateApiKeyPayload, CreateApiKeyResponse,
            // db_migrate
            MigrateRequest, MigrateResponse,
            // deployments
            CreateDeploymentPayload, CreateProjectDeploymentPayload,
            DeploySummary, DeploymentResponse, FunctionDeploySummaryEntry,
            // environments
            CloneEnvPayload, CreateEnvPayload, EnvironmentRow,
            // events
            CreateSubscriptionPayload, EventRow, EventSubscriptionRow,
            PublishEventPayload,
            // functions
            CreateFunctionPayload, CreateFunctionResponse, FunctionResponse,
            // gateway
            CorsPayload, CreateRoutePayload, MiddlewareCreatePayload,
            RateLimitPayload, RouteFullRow, RoutePayloadEntry, RouteRow,
            SyncRoutesPayload, UpdateRoutePayload,
            // queue
            CreateQueuePayload, DeadLetterJobRow, PublishMessagePayload,
            QueueConfigRow,
            // schedules
            CreateSchedulePayload, CronJobRow,
            // secrets
            CreateSecretRequest, SecretResponse, UpdateSecretRequest,
        );
    }
}
