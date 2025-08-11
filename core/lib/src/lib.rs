#[doc(inline)]
pub use rusteze_codegen::*;

pub use aws_lambda_events;
pub use lambda_runtime;
pub use serde_json;
pub use tokio;

// Core modules
pub mod aws_mock;
pub mod cloud;
pub mod config;
pub mod deployment;
pub mod error;
pub mod local_server;
pub mod logging;
pub mod messaging;
pub mod models;
pub mod policy;
pub mod retry;

// Re-export commonly used types
pub use aws_mock::{MockAwsCloudClients, MockMessagingClient};
pub use cloud::{
    AccessControlClient, ApiGatewayClient, CloudClients, CloudError, DefaultResourcePlanner,
    FileStateManager, MessagingClient, PlanningError, ResourcePlanner, ServerlessClient,
    StateError, StateManager,
};
pub use config::{ConfigError, DeploymentConfig, LambdaConfig, PubSubConfig, RustezeConfig};
pub use deployment::{
    ApiGatewayDeploymentStatus, ApiGatewayStatus, BuildConfiguration, DeploymentEngine,
    DeploymentError, DeploymentResult, DeploymentSummary, PackageFormat,
};
pub use error::{RustezeError, RustezeResult};
pub use local_server::{HttpRequest, HttpResponse, LocalServer, RouteHandler};
pub use logging::{
    AuditAction, AuditEntry, DeploymentLogEntry, DeploymentLogger, DeploymentSummaryReport,
    DeploymentSummaryStatus, OperationStatus, ResourceSummary, TimedOperation,
};
pub use messaging::{
    QueueManagementError, QueueManager, QueueRetryConfiguration, TopicConfiguration,
    TopicManagementError, TopicManager,
};
pub use models::{
    AccessPolicyPlan, AccessRoleState, ApiGatewayDrift, ApiGatewayPlan, ApiGatewayState,
    ConflictResolutionStrategy, DeploymentState, DriftDetectionResult, FunctionDrift, HandlerType,
    MessageQueuePlan, MessageQueueState, MessageTopicPlan, MessageTopicState, QueueDrift,
    ReconciliationOptions, ResourceDriftType, ResourcePlan, RouteMetadata, STATE_FILE_VERSION,
    ServerlessFunctionPlan, ServerlessFunctionState, StateFile, StateMetadata, StateMigrationError,
    StateValidationError, TopicDrift,
};
pub use policy::{PolicyManagementError, PolicyManager, PolicyTemplate};
pub use retry::{
    CircuitBreaker, CircuitBreakerConfig, CircuitBreakerState, RetryConfig, RetryExecutor,
    retry_cloud_operation, retry_network_operation, retry_operation, retry_state_operation,
    with_timeout,
};
