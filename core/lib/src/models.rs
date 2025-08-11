use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(test)]
mod tests;

/// Current state file format version for migration support
pub const STATE_FILE_VERSION: u32 = 1;

/// State file wrapper that includes version information for migration support
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateFile {
    pub version: u32,
    pub state: DeploymentState,
}

impl StateFile {
    /// Create a new state file with the current version
    pub fn new(state: DeploymentState) -> Self {
        Self {
            version: STATE_FILE_VERSION,
            state,
        }
    }

    /// Validate the state file structure and content
    pub fn validate(&self) -> Result<(), StateValidationError> {
        // Check version compatibility
        if self.version > STATE_FILE_VERSION {
            return Err(StateValidationError::UnsupportedVersion {
                found: self.version,
                supported: STATE_FILE_VERSION,
            });
        }

        // Validate metadata
        self.state.metadata.validate()?;

        // Validate serverless functions
        for function in &self.state.serverless_functions {
            function.validate()?;
        }

        // Validate API Gateway if present
        if let Some(ref api_gateway) = self.state.api_gateway {
            api_gateway.validate()?;
        }

        // Validate message topics
        for topic in &self.state.message_topics {
            topic.validate()?;
        }

        // Validate message queues
        for queue in &self.state.message_queues {
            queue.validate()?;
        }

        // Validate access roles
        self.state.access_roles.validate()?;

        Ok(())
    }

    /// Migrate state file to current version if needed
    pub fn migrate(self) -> Result<Self, StateMigrationError> {
        match self.version {
            1 => Ok(self), // Current version, no migration needed
            v if v < STATE_FILE_VERSION => {
                // Future migration logic would go here
                // For now, we only support version 1
                Ok(Self {
                    version: STATE_FILE_VERSION,
                    state: self.state,
                })
            }
            _ => Err(StateMigrationError::UnsupportedVersion(self.version)),
        }
    }

    /// Serialize to TOML string
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    /// Deserialize from TOML string
    pub fn from_toml(content: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(content)
    }
}

/// Cloud-agnostic resource management models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentState {
    pub metadata: StateMetadata,
    pub api_gateway: Option<ApiGatewayState>,
    pub serverless_functions: Vec<ServerlessFunctionState>,
    pub message_topics: Vec<MessageTopicState>,
    pub message_queues: Vec<MessageQueueState>,
    pub access_roles: AccessRoleState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateMetadata {
    pub service_name: String,
    pub last_deployment: DateTime<Utc>,
    pub deployment_id: String,
    pub provider: String,
    pub region: String,
    /// Configuration snapshot from the last deployment for change detection
    pub last_config_hash: Option<String>,
    /// Environment used in the last deployment
    pub last_environment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiGatewayState {
    pub api_id: String,
    pub api_url: String,
    pub stage: String,
    pub resource_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerlessFunctionState {
    pub name: String,
    pub resource_id: String,
    pub runtime: String,
    pub memory_size: u32,
    pub timeout: Option<u32>,
    pub environment_variables: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageTopicState {
    pub name: String,
    pub resource_id: String,
    pub display_name: Option<String>,
    pub dead_letter_queue_arn: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageQueueState {
    pub name: String,
    pub resource_id: String,
    pub topic_subscription: Option<String>,
    pub visibility_timeout_seconds: Option<u32>,
    pub message_retention_period: Option<u32>,
    pub dead_letter_queue: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessRoleState {
    pub function_execution_role: String,
    pub additional_policies: Vec<String>,
}

/// Deployment metadata models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteMetadata {
    pub function_name: String,
    pub method: String,
    pub path: String,
    pub handler_type: HandlerType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum HandlerType {
    HttpRoute,
    Publisher { topic: String },
    Subscriber { topic: String, queue: String },
}

/// Resource planning models
#[derive(Debug, Clone)]
pub struct ResourcePlan {
    pub serverless_functions: Vec<ServerlessFunctionPlan>,
    pub api_gateway: Option<ApiGatewayPlan>,
    pub message_topics: Vec<MessageTopicPlan>,
    pub message_queues: Vec<MessageQueuePlan>,
    pub access_policies: Vec<AccessPolicyPlan>,
}

#[derive(Debug, Clone)]
pub struct ServerlessFunctionPlan {
    pub name: String,
    pub handler: String,
    pub runtime: String,
    pub memory_size: u32,
    pub timeout: u32,
    pub environment_variables: HashMap<String, String>,
    pub route_metadata: Option<RouteMetadata>,
}

#[derive(Debug, Clone)]
pub struct ApiGatewayPlan {
    pub name: String,
    pub stage: String,
    pub routes: Vec<ApiRoutePlan>,
}

#[derive(Debug, Clone)]
pub struct ApiRoutePlan {
    pub method: String,
    pub path: String,
    pub function_name: String,
}

#[derive(Debug, Clone)]
pub struct MessageTopicPlan {
    pub name: String,
    pub display_name: Option<String>,
    pub dead_letter_queue_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MessageQueuePlan {
    pub name: String,
    pub topic_name: String,
    pub visibility_timeout_seconds: u32,
    pub message_retention_period: u32,
    pub max_receive_count: u32,
    pub enable_dead_letter_queue: bool,
}

#[derive(Debug, Clone)]
pub struct AccessPolicyPlan {
    pub name: String,
    pub policy_document: String,
    pub attached_to: Vec<String>,
}

/// State validation errors
#[derive(Debug, thiserror::Error)]
pub enum StateValidationError {
    #[error("Unsupported state file version: found {found}, supported {supported}")]
    UnsupportedVersion { found: u32, supported: u32 },

    #[error("Invalid service name: {0}")]
    InvalidServiceName(String),

    #[error("Invalid deployment ID: {0}")]
    InvalidDeploymentId(String),

    #[error("Invalid provider: {0}")]
    InvalidProvider(String),

    #[error("Invalid region: {0}")]
    InvalidRegion(String),

    #[error("Invalid resource ID: {0}")]
    InvalidResourceId(String),

    #[error("Invalid function name: {0}")]
    InvalidFunctionName(String),

    #[error("Invalid runtime: {0}")]
    InvalidRuntime(String),

    #[error("Invalid memory size: {0}")]
    InvalidMemorySize(u32),

    #[error("Invalid timeout: {0}")]
    InvalidTimeout(u32),

    #[error("Invalid API Gateway configuration: {0}")]
    InvalidApiGateway(String),

    #[error("Invalid topic name: {0}")]
    InvalidTopicName(String),

    #[error("Invalid queue name: {0}")]
    InvalidQueueName(String),

    #[error("Invalid role ARN: {0}")]
    InvalidRoleArn(String),
}

/// State migration errors
#[derive(Debug, thiserror::Error)]
pub enum StateMigrationError {
    #[error("Unsupported version for migration: {0}")]
    UnsupportedVersion(u32),

    #[error("Migration failed: {0}")]
    MigrationFailed(String),
}
// Validation implementations for state structs
impl StateMetadata {
    pub fn validate(&self) -> Result<(), StateValidationError> {
        // Validate service name
        if self.service_name.is_empty() {
            return Err(StateValidationError::InvalidServiceName(
                "Service name cannot be empty".to_string(),
            ));
        }

        if !self
            .service_name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err(StateValidationError::InvalidServiceName(
                "Service name can only contain alphanumeric characters, hyphens, and underscores"
                    .to_string(),
            ));
        }

        // Validate deployment ID
        if self.deployment_id.is_empty() {
            return Err(StateValidationError::InvalidDeploymentId(
                "Deployment ID cannot be empty".to_string(),
            ));
        }

        // Validate provider
        if self.provider.is_empty() {
            return Err(StateValidationError::InvalidProvider(
                "Provider cannot be empty".to_string(),
            ));
        }

        // Validate region
        if self.region.is_empty() {
            return Err(StateValidationError::InvalidRegion(
                "Region cannot be empty".to_string(),
            ));
        }

        Ok(())
    }
}

impl ServerlessFunctionState {
    pub fn validate(&self) -> Result<(), StateValidationError> {
        // Validate function name
        if self.name.is_empty() {
            return Err(StateValidationError::InvalidFunctionName(
                "Function name cannot be empty".to_string(),
            ));
        }

        // Validate resource ID
        if self.resource_id.is_empty() {
            return Err(StateValidationError::InvalidResourceId(
                "Function resource ID cannot be empty".to_string(),
            ));
        }

        // Validate runtime
        if self.runtime.is_empty() {
            return Err(StateValidationError::InvalidRuntime(
                "Runtime cannot be empty".to_string(),
            ));
        }

        // Validate memory size (AWS Lambda limits: 128 MB to 10,240 MB)
        if self.memory_size < 128 || self.memory_size > 10240 {
            return Err(StateValidationError::InvalidMemorySize(self.memory_size));
        }

        // Validate timeout if present (AWS Lambda limit: 1 to 900 seconds)
        if let Some(timeout) = self.timeout {
            if timeout == 0 || timeout > 900 {
                return Err(StateValidationError::InvalidTimeout(timeout));
            }
        }

        Ok(())
    }
}

impl ApiGatewayState {
    pub fn validate(&self) -> Result<(), StateValidationError> {
        // Validate API ID
        if self.api_id.is_empty() {
            return Err(StateValidationError::InvalidApiGateway(
                "API ID cannot be empty".to_string(),
            ));
        }

        // Validate API URL
        if self.api_url.is_empty() {
            return Err(StateValidationError::InvalidApiGateway(
                "API URL cannot be empty".to_string(),
            ));
        }

        // Basic URL validation
        if !self.api_url.starts_with("https://") {
            return Err(StateValidationError::InvalidApiGateway(
                "API URL must use HTTPS".to_string(),
            ));
        }

        // Validate stage
        if self.stage.is_empty() {
            return Err(StateValidationError::InvalidApiGateway(
                "Stage cannot be empty".to_string(),
            ));
        }

        // Validate resource ID
        if self.resource_id.is_empty() {
            return Err(StateValidationError::InvalidResourceId(
                "API Gateway resource ID cannot be empty".to_string(),
            ));
        }

        Ok(())
    }
}

impl MessageTopicState {
    pub fn validate(&self) -> Result<(), StateValidationError> {
        // Validate topic name
        if self.name.is_empty() {
            return Err(StateValidationError::InvalidTopicName(
                "Topic name cannot be empty".to_string(),
            ));
        }

        // Validate resource ID
        if self.resource_id.is_empty() {
            return Err(StateValidationError::InvalidResourceId(
                "Topic resource ID cannot be empty".to_string(),
            ));
        }

        Ok(())
    }
}

impl MessageQueueState {
    pub fn validate(&self) -> Result<(), StateValidationError> {
        // Validate queue name
        if self.name.is_empty() {
            return Err(StateValidationError::InvalidQueueName(
                "Queue name cannot be empty".to_string(),
            ));
        }

        // Validate resource ID
        if self.resource_id.is_empty() {
            return Err(StateValidationError::InvalidResourceId(
                "Queue resource ID cannot be empty".to_string(),
            ));
        }

        // Validate visibility timeout if present (SQS limits: 0 to 43,200 seconds)
        if let Some(timeout) = self.visibility_timeout_seconds {
            if timeout > 43200 {
                return Err(StateValidationError::InvalidTimeout(timeout));
            }
        }

        // Validate message retention period if present (SQS limits: 60 to 1,209,600 seconds)
        if let Some(retention) = self.message_retention_period {
            if retention < 60 || retention > 1209600 {
                return Err(StateValidationError::InvalidTimeout(retention));
            }
        }

        Ok(())
    }
}

impl AccessRoleState {
    pub fn validate(&self) -> Result<(), StateValidationError> {
        // Validate function execution role ARN
        if self.function_execution_role.is_empty() {
            return Err(StateValidationError::InvalidRoleArn(
                "Function execution role cannot be empty".to_string(),
            ));
        }

        // Basic ARN validation for AWS IAM roles
        if !self.function_execution_role.starts_with("arn:aws:iam::") {
            return Err(StateValidationError::InvalidRoleArn(
                "Invalid IAM role ARN format".to_string(),
            ));
        }

        Ok(())
    }
}
/// Resource drift detection and reconciliation models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftDetectionResult {
    pub has_drift: bool,
    pub function_drifts: Vec<FunctionDrift>,
    pub topic_drifts: Vec<TopicDrift>,
    pub queue_drifts: Vec<QueueDrift>,
    pub api_gateway_drift: Option<ApiGatewayDrift>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDrift {
    pub function_name: String,
    pub drift_type: ResourceDriftType,
    pub expected: Option<ServerlessFunctionState>,
    pub actual: Option<ServerlessFunctionState>,
    pub differences: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicDrift {
    pub topic_name: String,
    pub drift_type: ResourceDriftType,
    pub expected: Option<MessageTopicState>,
    pub actual: Option<MessageTopicState>,
    pub differences: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueDrift {
    pub queue_name: String,
    pub drift_type: ResourceDriftType,
    pub expected: Option<MessageQueueState>,
    pub actual: Option<MessageQueueState>,
    pub differences: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiGatewayDrift {
    pub drift_type: ResourceDriftType,
    pub expected: Option<ApiGatewayState>,
    pub actual: Option<ApiGatewayState>,
    pub differences: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResourceDriftType {
    /// Resource exists in state but not in cloud
    Missing,
    /// Resource exists in cloud but not in state
    Unexpected,
    /// Resource exists in both but with different configuration
    Modified,
    /// Resource matches expected state
    InSync,
}

/// Conflict resolution strategies for state reconciliation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConflictResolutionStrategy {
    /// Use the cloud resource as the source of truth
    PreferCloud,
    /// Use the state file as the source of truth
    PreferState,
    /// Prompt user for manual resolution
    Manual,
    /// Skip conflicted resources
    Skip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconciliationOptions {
    pub conflict_resolution: ConflictResolutionStrategy,
    pub create_missing_resources: bool,
    pub remove_unexpected_resources: bool,
    pub update_modified_resources: bool,
    pub dry_run: bool,
}

impl Default for ReconciliationOptions {
    fn default() -> Self {
        Self {
            conflict_resolution: ConflictResolutionStrategy::PreferCloud,
            create_missing_resources: false,
            remove_unexpected_resources: false,
            update_modified_resources: true,
            dry_run: false,
        }
    }
}
