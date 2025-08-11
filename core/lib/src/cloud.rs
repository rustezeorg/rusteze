use crate::models::*;
use crate::retry::*;
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::AsyncWriteExt;

#[cfg(test)]
mod tests;

/// Cloud-agnostic client abstractions for resource management
#[async_trait]
pub trait CloudClients: Send + Sync {
    fn serverless(&self) -> &dyn ServerlessClient;
    fn api_gateway(&self) -> &dyn ApiGatewayClient;
    fn messaging(&self) -> &dyn MessagingClient;
    fn access_control(&self) -> &dyn AccessControlClient;
}

/// Serverless function management client
#[async_trait]
pub trait ServerlessClient: Send + Sync {
    async fn create_function(
        &self,
        plan: &ServerlessFunctionPlan,
    ) -> Result<ServerlessFunctionState, CloudError>;
    async fn update_function(
        &self,
        name: &str,
        plan: &ServerlessFunctionPlan,
    ) -> Result<ServerlessFunctionState, CloudError>;
    async fn delete_function(&self, name: &str) -> Result<(), CloudError>;
    async fn get_function(&self, name: &str)
    -> Result<Option<ServerlessFunctionState>, CloudError>;
    async fn list_functions(&self) -> Result<Vec<ServerlessFunctionState>, CloudError>;
    async fn update_function_configuration(
        &self,
        name: &str,
        config: &FunctionConfiguration,
    ) -> Result<(), CloudError>;
}

/// API Gateway management client
#[async_trait]
pub trait ApiGatewayClient: Send + Sync {
    async fn create_api(&self, plan: &ApiGatewayPlan) -> Result<ApiGatewayState, CloudError>;
    async fn update_api(
        &self,
        api_id: &str,
        plan: &ApiGatewayPlan,
    ) -> Result<ApiGatewayState, CloudError>;
    async fn delete_api(&self, api_id: &str) -> Result<(), CloudError>;
    async fn get_api(&self, api_id: &str) -> Result<Option<ApiGatewayState>, CloudError>;
    async fn deploy_api(&self, api_id: &str, stage: &str) -> Result<String, CloudError>;
    async fn create_route(&self, api_id: &str, route: &ApiRoutePlan) -> Result<(), CloudError>;
    async fn delete_route(&self, api_id: &str, method: &str, path: &str) -> Result<(), CloudError>;
}

/// Messaging (pub/sub) management client
#[async_trait]
pub trait MessagingClient: Send + Sync {
    async fn create_topic(&self, plan: &MessageTopicPlan) -> Result<MessageTopicState, CloudError>;
    async fn delete_topic(&self, topic_id: &str) -> Result<(), CloudError>;
    async fn get_topic(&self, topic_id: &str) -> Result<Option<MessageTopicState>, CloudError>;
    async fn list_topics(&self) -> Result<Vec<MessageTopicState>, CloudError>;

    async fn create_queue(&self, plan: &MessageQueuePlan) -> Result<MessageQueueState, CloudError>;
    async fn delete_queue(&self, queue_id: &str) -> Result<(), CloudError>;
    async fn get_queue(&self, queue_id: &str) -> Result<Option<MessageQueueState>, CloudError>;
    async fn list_queues(&self) -> Result<Vec<MessageQueueState>, CloudError>;

    async fn subscribe_queue_to_topic(
        &self,
        queue_id: &str,
        topic_id: &str,
    ) -> Result<String, CloudError>;
    async fn unsubscribe_queue_from_topic(&self, subscription_id: &str) -> Result<(), CloudError>;

    async fn publish_message(
        &self,
        topic_id: &str,
        message: &str,
        attributes: Option<HashMap<String, String>>,
    ) -> Result<String, CloudError>;
}

/// Access control and IAM management client
#[async_trait]
pub trait AccessControlClient: Send + Sync {
    async fn create_execution_role(
        &self,
        service_name: &str,
        policies: &[AccessPolicyPlan],
    ) -> Result<String, CloudError>;
    async fn update_execution_role(
        &self,
        role_arn: &str,
        policies: &[AccessPolicyPlan],
    ) -> Result<(), CloudError>;
    async fn delete_execution_role(&self, role_arn: &str) -> Result<(), CloudError>;
    async fn get_execution_role(
        &self,
        role_arn: &str,
    ) -> Result<Option<AccessRoleState>, CloudError>;

    async fn create_policy(&self, plan: &AccessPolicyPlan) -> Result<String, CloudError>;
    async fn delete_policy(&self, policy_arn: &str) -> Result<(), CloudError>;
    async fn attach_policy_to_role(
        &self,
        role_arn: &str,
        policy_arn: &str,
    ) -> Result<(), CloudError>;
    async fn detach_policy_from_role(
        &self,
        role_arn: &str,
        policy_arn: &str,
    ) -> Result<(), CloudError>;
}

/// Configuration for serverless functions
#[derive(Debug, Clone)]
pub struct FunctionConfiguration {
    pub memory_size: Option<u32>,
    pub timeout: Option<u32>,
    pub environment_variables: Option<HashMap<String, String>>,
    pub description: Option<String>,
}

/// Cloud provider errors
#[derive(Debug, thiserror::Error)]
pub enum CloudError {
    #[error("Authentication error: {0}")]
    Authentication(String),

    #[error("Authorization error: {0}")]
    Authorization(String),

    #[error("Resource not found: {0}")]
    ResourceNotFound(String),

    #[error("Resource already exists: {0}")]
    ResourceAlreadyExists(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Rate limit exceeded: {0}")]
    RateLimit(String),

    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),

    #[error("Unknown cloud provider error: {0}")]
    Unknown(String),
}

impl CloudError {
    /// Check if the error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            CloudError::Network(_) | CloudError::RateLimit(_) | CloudError::ServiceUnavailable(_)
        )
    }

    /// Get suggested remediation steps for the error
    pub fn remediation_steps(&self) -> Vec<String> {
        match self {
            CloudError::Authentication(_) => vec![
                "Check your cloud provider credentials".to_string(),
                "Ensure your access keys are valid and not expired".to_string(),
                "Verify your authentication configuration".to_string(),
            ],
            CloudError::Authorization(_) => vec![
                "Check your IAM permissions".to_string(),
                "Ensure your user/role has the required policies attached".to_string(),
                "Verify resource-level permissions".to_string(),
            ],
            CloudError::ResourceNotFound(_) => vec![
                "Verify the resource name and identifier".to_string(),
                "Check if the resource exists in the correct region".to_string(),
                "Ensure you have permission to access the resource".to_string(),
            ],
            CloudError::ResourceAlreadyExists(_) => vec![
                "Use a different resource name".to_string(),
                "Delete the existing resource if it's no longer needed".to_string(),
                "Update the existing resource instead of creating a new one".to_string(),
            ],
            CloudError::InvalidConfiguration(_) => vec![
                "Review your rusteze.toml configuration".to_string(),
                "Check the resource configuration parameters".to_string(),
                "Validate configuration against provider requirements".to_string(),
            ],
            CloudError::Network(_) => vec![
                "Check your internet connection".to_string(),
                "Verify network connectivity to cloud provider endpoints".to_string(),
                "Try again in a few moments".to_string(),
            ],
            CloudError::RateLimit(_) => vec![
                "Wait before retrying the operation".to_string(),
                "Reduce the frequency of API calls".to_string(),
                "Consider implementing exponential backoff".to_string(),
            ],
            CloudError::ServiceUnavailable(_) => vec![
                "Check cloud provider service status".to_string(),
                "Try again later".to_string(),
                "Consider using a different region if available".to_string(),
            ],
            CloudError::Unknown(_) => vec![
                "Check cloud provider documentation".to_string(),
                "Review error logs for more details".to_string(),
                "Contact support if the issue persists".to_string(),
            ],
        }
    }
}

/// Resource discovery and planning
pub trait ResourcePlanner: Send + Sync {
    fn analyze_code(&self, metadata: &[RouteMetadata]) -> Result<ResourcePlan, PlanningError>;
    fn validate_plan(&self, plan: &ResourcePlan) -> Result<(), PlanningError>;
    fn calculate_dependencies(&self, plan: &ResourcePlan) -> Result<Vec<String>, PlanningError>;
}

/// Default implementation of ResourcePlanner
pub struct DefaultResourcePlanner {
    service_name: String,
    default_runtime: String,
    default_memory_size: u32,
    default_timeout: u32,
}

impl DefaultResourcePlanner {
    /// Create a new DefaultResourcePlanner
    pub fn new(service_name: String) -> Self {
        Self {
            service_name,
            default_runtime: "provided.al2".to_string(),
            default_memory_size: 256,
            default_timeout: 30,
        }
    }

    /// Create a new DefaultResourcePlanner with custom defaults
    pub fn with_defaults(
        service_name: String,
        runtime: String,
        memory_size: u32,
        timeout: u32,
    ) -> Self {
        Self {
            service_name,
            default_runtime: runtime,
            default_memory_size: memory_size,
            default_timeout: timeout,
        }
    }

    /// Generate resource name with service prefix
    fn generate_resource_name(&self, base_name: &str) -> String {
        format!("{}-{}", self.service_name, base_name)
    }

    /// Generate function name from route metadata
    fn generate_function_name(&self, route: &RouteMetadata) -> String {
        match &route.handler_type {
            HandlerType::HttpRoute => {
                // Convert path to function name: /users/{id} -> users_id
                let path_name = route
                    .path
                    .trim_start_matches('/')
                    .replace('/', "_")
                    .replace('{', "")
                    .replace('}', "");

                let method_name = route.method.to_lowercase();

                if path_name.is_empty() {
                    format!("{}_root", method_name)
                } else {
                    format!("{}_{}", method_name, path_name)
                }
            }
            HandlerType::Publisher { topic } => {
                format!("publish_{}", topic.replace('-', "_"))
            }
            HandlerType::Subscriber { queue, .. } => {
                format!("consume_{}", queue.replace('-', "_"))
            }
        }
    }

    /// Extract unique topics from route metadata
    fn extract_topics(&self, metadata: &[RouteMetadata]) -> Vec<String> {
        let mut topics = std::collections::HashSet::new();

        for route in metadata {
            match &route.handler_type {
                HandlerType::Publisher { topic } => {
                    topics.insert(topic.clone());
                }
                HandlerType::Subscriber { topic, .. } => {
                    topics.insert(topic.clone());
                }
                HandlerType::HttpRoute => {}
            }
        }

        topics.into_iter().collect()
    }

    /// Generate topic naming convention with service prefix
    fn generate_topic_name(&self, topic: &str) -> String {
        self.generate_resource_name(topic)
    }

    /// Generate dead letter queue name for a topic
    fn generate_dead_letter_queue_name(&self, topic: &str) -> String {
        format!("{}-dlq", self.generate_topic_name(topic))
    }

    /// Create topic plans with dead letter queue configuration
    fn create_topic_plans(&self, topics: &[String]) -> Vec<MessageTopicPlan> {
        topics
            .iter()
            .map(|topic| {
                let topic_name = self.generate_topic_name(topic);
                let dlq_name = self.generate_dead_letter_queue_name(topic);
                MessageTopicPlan {
                    name: topic_name.clone(),
                    display_name: Some(format!("{} Topic", topic)),
                    dead_letter_queue_name: Some(dlq_name),
                }
            })
            .collect()
    }

    /// Extract unique queues from route metadata
    fn extract_queues(&self, metadata: &[RouteMetadata]) -> Vec<(String, String)> {
        let mut queues = Vec::new();

        for route in metadata {
            if let HandlerType::Subscriber { topic, queue } = &route.handler_type {
                queues.push((queue.clone(), topic.clone()));
            }
        }

        queues
    }

    /// Generate access policies based on resource requirements
    fn generate_access_policies(&self, plan: &ResourcePlan) -> Vec<AccessPolicyPlan> {
        let mut policies = Vec::new();

        // SNS publish policy if there are topics
        if !plan.message_topics.is_empty() {
            let topic_arns: Vec<String> = plan
                .message_topics
                .iter()
                .map(|t| format!("arn:aws:sns:*:*:{}", self.generate_resource_name(&t.name)))
                .collect();

            let sns_policy = AccessPolicyPlan {
                name: format!("{}-sns-publish-policy", self.service_name),
                policy_document: format!(
                    r#"{{
                        "Version": "2012-10-17",
                        "Statement": [
                            {{
                                "Effect": "Allow",
                                "Action": [
                                    "sns:Publish"
                                ],
                                "Resource": [
                                    {}
                                ]
                            }}
                        ]
                    }}"#,
                    topic_arns
                        .iter()
                        .map(|arn| format!("\"{}\"", arn))
                        .collect::<Vec<_>>()
                        .join(",")
                ),
                attached_to: vec![format!("{}-execution-role", self.service_name)],
            };
            policies.push(sns_policy);
        }

        // SQS consume policy if there are queues
        if !plan.message_queues.is_empty() {
            let queue_arns: Vec<String> = plan
                .message_queues
                .iter()
                .map(|q| format!("arn:aws:sqs:*:*:{}", self.generate_resource_name(&q.name)))
                .collect();

            let sqs_policy = AccessPolicyPlan {
                name: format!("{}-sqs-consume-policy", self.service_name),
                policy_document: format!(
                    r#"{{
                        "Version": "2012-10-17",
                        "Statement": [
                            {{
                                "Effect": "Allow",
                                "Action": [
                                    "sqs:ReceiveMessage",
                                    "sqs:DeleteMessage",
                                    "sqs:GetQueueAttributes"
                                ],
                                "Resource": [
                                    {}
                                ]
                            }}
                        ]
                    }}"#,
                    queue_arns
                        .iter()
                        .map(|arn| format!("\"{}\"", arn))
                        .collect::<Vec<_>>()
                        .join(",")
                ),
                attached_to: vec![format!("{}-execution-role", self.service_name)],
            };
            policies.push(sqs_policy);
        }

        policies
    }
}

impl ResourcePlanner for DefaultResourcePlanner {
    fn analyze_code(&self, metadata: &[RouteMetadata]) -> Result<ResourcePlan, PlanningError> {
        let mut serverless_functions = Vec::new();
        let mut api_routes = Vec::new();
        let mut has_http_routes = false;

        // Process each route metadata to create serverless functions
        for route in metadata {
            let function_name = self.generate_function_name(route);
            let resource_function_name = self.generate_resource_name(&function_name);

            // Create environment variables for pub/sub functions
            let mut environment_variables = std::collections::HashMap::new();

            match &route.handler_type {
                HandlerType::HttpRoute => {
                    has_http_routes = true;
                    api_routes.push(ApiRoutePlan {
                        method: route.method.clone(),
                        path: route.path.clone(),
                        function_name: resource_function_name.clone(),
                    });
                }
                HandlerType::Publisher { topic } => {
                    let topic_resource_name = self.generate_resource_name(topic);
                    environment_variables.insert(
                        "TOPIC_ARN".to_string(),
                        format!("arn:aws:sns:us-east-1:123456789:{}", topic_resource_name),
                    );
                }
                HandlerType::Subscriber { topic, queue } => {
                    let topic_resource_name = self.generate_resource_name(topic);
                    let queue_resource_name = self.generate_resource_name(queue);
                    environment_variables.insert(
                        "TOPIC_ARN".to_string(),
                        format!("arn:aws:sns:us-east-1:123456789:{}", topic_resource_name),
                    );
                    environment_variables.insert(
                        "QUEUE_URL".to_string(),
                        format!(
                            "https://sqs.us-east-1.amazonaws.com/123456789/{}",
                            queue_resource_name
                        ),
                    );
                }
            }

            let function_plan = ServerlessFunctionPlan {
                name: resource_function_name,
                handler: route.function_name.clone(),
                runtime: self.default_runtime.clone(),
                memory_size: self.default_memory_size,
                timeout: self.default_timeout,
                environment_variables,
                route_metadata: Some(route.clone()),
            };

            serverless_functions.push(function_plan);
        }

        // Create API Gateway plan if there are HTTP routes
        let api_gateway = if has_http_routes {
            Some(ApiGatewayPlan {
                name: self.generate_resource_name("api"),
                stage: "prod".to_string(),
                routes: api_routes,
            })
        } else {
            None
        };

        // Create message topics with enhanced naming and configuration
        let topics = self.extract_topics(metadata);
        let message_topics = self.create_topic_plans(&topics);

        // Create message queues
        let queues = self.extract_queues(metadata);
        let message_queues = queues
            .into_iter()
            .map(|(queue, topic)| MessageQueuePlan {
                name: self.generate_resource_name(&queue),
                topic_name: self.generate_resource_name(&topic),
                visibility_timeout_seconds: 30,
                message_retention_period: 1209600, // 14 days
                max_receive_count: 3,
                enable_dead_letter_queue: true,
            })
            .collect();

        let plan = ResourcePlan {
            serverless_functions,
            api_gateway,
            message_topics,
            message_queues,
            access_policies: Vec::new(), // Will be populated in validate_plan
        };

        Ok(plan)
    }

    fn validate_plan(&self, plan: &ResourcePlan) -> Result<(), PlanningError> {
        // Validate function names are unique
        let mut function_names = std::collections::HashSet::new();
        for function in &plan.serverless_functions {
            if !function_names.insert(&function.name) {
                return Err(PlanningError::NamingConflict(format!(
                    "Duplicate function name: {}",
                    function.name
                )));
            }

            // Validate function configuration
            if function.name.is_empty() {
                return Err(PlanningError::InvalidRoute(
                    "Function name cannot be empty".to_string(),
                ));
            }

            if function.handler.is_empty() {
                return Err(PlanningError::InvalidRoute(
                    "Function handler cannot be empty".to_string(),
                ));
            }

            if function.memory_size < 128 || function.memory_size > 10240 {
                return Err(PlanningError::InvalidRoute(format!(
                    "Invalid memory size for function {}: {}",
                    function.name, function.memory_size
                )));
            }

            if function.timeout == 0 || function.timeout > 900 {
                return Err(PlanningError::InvalidRoute(format!(
                    "Invalid timeout for function {}: {}",
                    function.name, function.timeout
                )));
            }
        }

        // Validate API Gateway configuration if present
        if let Some(ref api_gateway) = plan.api_gateway {
            if api_gateway.name.is_empty() {
                return Err(PlanningError::InvalidRoute(
                    "API Gateway name cannot be empty".to_string(),
                ));
            }

            if api_gateway.stage.is_empty() {
                return Err(PlanningError::InvalidRoute(
                    "API Gateway stage cannot be empty".to_string(),
                ));
            }

            // Validate routes
            let mut route_keys = std::collections::HashSet::new();
            for route in &api_gateway.routes {
                let route_key = format!("{} {}", route.method, route.path);
                if !route_keys.insert(route_key.clone()) {
                    return Err(PlanningError::NamingConflict(format!(
                        "Duplicate API route: {}",
                        route_key
                    )));
                }

                // Validate that the function exists
                if !plan
                    .serverless_functions
                    .iter()
                    .any(|f| f.name == route.function_name)
                {
                    return Err(PlanningError::MissingDependency(format!(
                        "Function {} referenced by route {} {} not found",
                        route.function_name, route.method, route.path
                    )));
                }
            }
        }

        // Validate topic names are unique
        let mut topic_names = std::collections::HashSet::new();
        for topic in &plan.message_topics {
            if !topic_names.insert(&topic.name) {
                return Err(PlanningError::NamingConflict(format!(
                    "Duplicate topic name: {}",
                    topic.name
                )));
            }

            if topic.name.is_empty() {
                return Err(PlanningError::InvalidRoute(
                    "Topic name cannot be empty".to_string(),
                ));
            }
        }

        // Validate queue names are unique and reference valid topics
        let mut queue_names = std::collections::HashSet::new();
        for queue in &plan.message_queues {
            if !queue_names.insert(&queue.name) {
                return Err(PlanningError::NamingConflict(format!(
                    "Duplicate queue name: {}",
                    queue.name
                )));
            }

            if queue.name.is_empty() {
                return Err(PlanningError::InvalidRoute(
                    "Queue name cannot be empty".to_string(),
                ));
            }

            // Validate that the topic exists
            if !plan
                .message_topics
                .iter()
                .any(|t| t.name == queue.topic_name)
            {
                return Err(PlanningError::MissingDependency(format!(
                    "Topic {} referenced by queue {} not found",
                    queue.topic_name, queue.name
                )));
            }

            // Validate queue configuration
            if queue.visibility_timeout_seconds > 43200 {
                return Err(PlanningError::InvalidRoute(format!(
                    "Invalid visibility timeout for queue {}: {}",
                    queue.name, queue.visibility_timeout_seconds
                )));
            }

            if queue.message_retention_period < 60 || queue.message_retention_period > 1209600 {
                return Err(PlanningError::InvalidRoute(format!(
                    "Invalid message retention period for queue {}: {}",
                    queue.name, queue.message_retention_period
                )));
            }
        }

        Ok(())
    }

    fn calculate_dependencies(&self, plan: &ResourcePlan) -> Result<Vec<String>, PlanningError> {
        let mut dependencies = Vec::new();

        // IAM roles must be created first
        dependencies.push("access_roles".to_string());

        // Topics must be created before queues (for subscriptions)
        if !plan.message_topics.is_empty() {
            dependencies.push("message_topics".to_string());
        }

        // Queues depend on topics
        if !plan.message_queues.is_empty() {
            if plan.message_topics.is_empty() {
                return Err(PlanningError::MissingDependency(
                    "Message queues require topics to be defined".to_string(),
                ));
            }
            dependencies.push("message_queues".to_string());
        }

        // Functions can be created after IAM roles and messaging infrastructure
        if !plan.serverless_functions.is_empty() {
            dependencies.push("serverless_functions".to_string());
        }

        // API Gateway must be created after functions
        if plan.api_gateway.is_some() {
            if plan.serverless_functions.is_empty() {
                return Err(PlanningError::MissingDependency(
                    "API Gateway requires serverless functions to be defined".to_string(),
                ));
            }
            dependencies.push("api_gateway".to_string());
        }

        Ok(dependencies)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PlanningError {
    #[error("Invalid route configuration: {0}")]
    InvalidRoute(String),

    #[error("Missing dependency: {0}")]
    MissingDependency(String),

    #[error("Circular dependency detected: {0}")]
    CircularDependency(String),

    #[error("Resource naming conflict: {0}")]
    NamingConflict(String),

    #[error("Unsupported configuration: {0}")]
    UnsupportedConfiguration(String),
}

/// State management operations
#[async_trait]
pub trait StateManager: Send + Sync {
    async fn load_state(&self) -> Result<Option<DeploymentState>, StateError>;
    async fn save_state(&self, state: &DeploymentState) -> Result<(), StateError>;
    async fn reconcile_with_cloud(
        &self,
        cloud_clients: &dyn CloudClients,
    ) -> Result<DeploymentState, StateError>;
    async fn backup_state(&self) -> Result<String, StateError>;
    async fn restore_state(&self, backup_path: &str) -> Result<DeploymentState, StateError>;

    /// Detect drift between state file and actual cloud resources
    async fn detect_drift(
        &self,
        cloud_clients: &dyn CloudClients,
    ) -> Result<DriftDetectionResult, StateError>;

    /// Reconcile with advanced options and conflict resolution
    async fn reconcile_with_options(
        &self,
        cloud_clients: &dyn CloudClients,
        options: &ReconciliationOptions,
    ) -> Result<DeploymentState, StateError>;
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("State file not found")]
    FileNotFound,

    #[error("Failed to read state file: {0}")]
    FileRead(String),

    #[error("Failed to write state file: {0}")]
    FileWrite(String),

    #[error("State file is corrupted: {0}")]
    Corrupted(String),

    #[error("State reconciliation failed: {0}")]
    ReconciliationFailed(String),

    #[error("Backup operation failed: {0}")]
    BackupFailed(String),

    #[error("Restore operation failed: {0}")]
    RestoreFailed(String),
}
/// File-based state manager implementation
pub struct FileStateManager {
    state_file_path: PathBuf,
    backup_dir: PathBuf,
}

impl FileStateManager {
    /// Create a new FileStateManager with the specified state file path
    pub fn new<P: AsRef<Path>>(state_file_path: P) -> Self {
        let state_file_path = state_file_path.as_ref().to_path_buf();
        let backup_dir = state_file_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(".rusteze-backups");

        Self {
            state_file_path,
            backup_dir,
        }
    }

    /// Create a new FileStateManager with default state file path
    pub fn default() -> Self {
        Self::new(".rusteze-state.toml")
    }

    /// Ensure backup directory exists
    async fn ensure_backup_dir(&self) -> Result<(), StateError> {
        if !self.backup_dir.exists() {
            fs::create_dir_all(&self.backup_dir).await.map_err(|e| {
                StateError::BackupFailed(format!("Failed to create backup directory: {}", e))
            })?;
        }
        Ok(())
    }

    /// Generate backup file path with timestamp
    fn generate_backup_path(&self) -> PathBuf {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        self.backup_dir
            .join(format!("state_backup_{}.toml", timestamp))
    }

    /// Perform atomic write operation using a temporary file
    async fn atomic_write(&self, content: &str) -> Result<(), StateError> {
        let temp_path = self.state_file_path.with_extension("tmp");

        // Write to temporary file first
        let mut temp_file = fs::File::create(&temp_path).await.map_err(|e| {
            StateError::FileWrite(format!("Failed to create temporary file: {}", e))
        })?;

        temp_file.write_all(content.as_bytes()).await.map_err(|e| {
            StateError::FileWrite(format!("Failed to write to temporary file: {}", e))
        })?;

        temp_file
            .sync_all()
            .await
            .map_err(|e| StateError::FileWrite(format!("Failed to sync temporary file: {}", e)))?;

        // Atomically move temporary file to final location
        fs::rename(&temp_path, &self.state_file_path)
            .await
            .map_err(|e| StateError::FileWrite(format!("Failed to move temporary file: {}", e)))?;

        Ok(())
    }

    /// Create backup of current state file if it exists
    async fn create_backup(&self) -> Result<Option<String>, StateError> {
        if !self.state_file_path.exists() {
            return Ok(None);
        }

        self.ensure_backup_dir().await?;
        let backup_path = self.generate_backup_path();

        fs::copy(&self.state_file_path, &backup_path)
            .await
            .map_err(|e| StateError::BackupFailed(format!("Failed to create backup: {}", e)))?;

        Ok(Some(backup_path.to_string_lossy().to_string()))
    }

    /// Compare two serverless function states and detect differences
    fn compare_functions(
        expected: &ServerlessFunctionState,
        actual: &ServerlessFunctionState,
    ) -> Vec<String> {
        let mut differences = Vec::new();

        if expected.runtime != actual.runtime {
            differences.push(format!(
                "Runtime: expected '{}', actual '{}'",
                expected.runtime, actual.runtime
            ));
        }

        if expected.memory_size != actual.memory_size {
            differences.push(format!(
                "Memory size: expected {}, actual {}",
                expected.memory_size, actual.memory_size
            ));
        }

        if expected.timeout != actual.timeout {
            differences.push(format!(
                "Timeout: expected {:?}, actual {:?}",
                expected.timeout, actual.timeout
            ));
        }

        if expected.environment_variables != actual.environment_variables {
            differences.push("Environment variables differ".to_string());
        }

        differences
    }

    /// Compare two message topic states and detect differences
    fn compare_topics(expected: &MessageTopicState, actual: &MessageTopicState) -> Vec<String> {
        let mut differences = Vec::new();

        if expected.display_name != actual.display_name {
            differences.push(format!(
                "Display name: expected {:?}, actual {:?}",
                expected.display_name, actual.display_name
            ));
        }

        differences
    }

    /// Compare two message queue states and detect differences
    fn compare_queues(expected: &MessageQueueState, actual: &MessageQueueState) -> Vec<String> {
        let mut differences = Vec::new();

        if expected.topic_subscription != actual.topic_subscription {
            differences.push(format!(
                "Topic subscription: expected {:?}, actual {:?}",
                expected.topic_subscription, actual.topic_subscription
            ));
        }

        if expected.visibility_timeout_seconds != actual.visibility_timeout_seconds {
            differences.push(format!(
                "Visibility timeout: expected {:?}, actual {:?}",
                expected.visibility_timeout_seconds, actual.visibility_timeout_seconds
            ));
        }

        if expected.message_retention_period != actual.message_retention_period {
            differences.push(format!(
                "Message retention period: expected {:?}, actual {:?}",
                expected.message_retention_period, actual.message_retention_period
            ));
        }

        if expected.dead_letter_queue != actual.dead_letter_queue {
            differences.push(format!(
                "Dead letter queue: expected {:?}, actual {:?}",
                expected.dead_letter_queue, actual.dead_letter_queue
            ));
        }

        differences
    }
}

#[async_trait]
impl StateManager for FileStateManager {
    async fn load_state(&self) -> Result<Option<DeploymentState>, StateError> {
        if !self.state_file_path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&self.state_file_path)
            .await
            .map_err(|e| StateError::FileRead(format!("Failed to read state file: {}", e)))?;

        let state_file = StateFile::from_toml(&content)
            .map_err(|e| StateError::Corrupted(format!("Failed to parse state file: {}", e)))?;

        // Validate the state file
        state_file
            .validate()
            .map_err(|e| StateError::Corrupted(format!("State validation failed: {}", e)))?;

        // Store original version for comparison
        let original_version = state_file.version;

        // Migrate if necessary
        let migrated_state_file = state_file
            .migrate()
            .map_err(|e| StateError::Corrupted(format!("State migration failed: {}", e)))?;

        // If migration occurred, save the migrated state
        if migrated_state_file.version != original_version {
            self.save_state(&migrated_state_file.state).await?;
        }

        Ok(Some(migrated_state_file.state))
    }

    async fn save_state(&self, state: &DeploymentState) -> Result<(), StateError> {
        // Use retry logic for state operations
        retry_state_operation(|| async {
            // Create backup before saving new state
            self.create_backup().await.map_err(|e| {
                crate::error::RustezeError::state(format!("Failed to create backup: {}", e))
            })?;

            let state_file = StateFile::new(state.clone());

            // Validate before saving
            state_file.validate().map_err(|e| {
                crate::error::RustezeError::validation(format!("State validation failed: {}", e))
            })?;

            let content = state_file.to_toml().map_err(|e| {
                crate::error::RustezeError::serialization(format!(
                    "Failed to serialize state: {}",
                    e
                ))
            })?;

            self.atomic_write(&content).await.map_err(|e| {
                crate::error::RustezeError::state(format!("Failed to write state file: {}", e))
            })?;

            Ok(())
        })
        .await
        .map_err(|e| match e {
            crate::error::RustezeError::State {
                source: Some(state_err),
                ..
            } => state_err,
            _ => StateError::FileWrite(e.to_string()),
        })
    }

    async fn reconcile_with_cloud(
        &self,
        cloud_clients: &dyn CloudClients,
    ) -> Result<DeploymentState, StateError> {
        // Load current state or create empty state
        let mut current_state = self.load_state().await?.unwrap_or_else(|| DeploymentState {
            metadata: StateMetadata {
                service_name: "unknown".to_string(),
                last_deployment: chrono::Utc::now(),
                deployment_id: "reconciliation".to_string(),
                provider: "aws".to_string(),
                region: "us-east-1".to_string(),
                last_config_hash: None,
                last_environment: None,
            },
            api_gateway: None,
            serverless_functions: Vec::new(),
            message_topics: Vec::new(),
            message_queues: Vec::new(),
            access_roles: AccessRoleState {
                function_execution_role: "arn:aws:iam::123456789:role/reconcile-role".to_string(),
                additional_policies: Vec::new(),
            },
        });

        // Reconcile serverless functions with retry logic
        let cloud_functions = retry_cloud_operation(|| async {
            cloud_clients
                .serverless()
                .list_functions()
                .await
                .map_err(|e| {
                    crate::error::RustezeError::cloud(format!("Failed to list functions: {}", e))
                })
        })
        .await
        .map_err(|e| StateError::ReconciliationFailed(e.to_string()))?;

        // Update state with actual cloud resources
        current_state.serverless_functions = cloud_functions;

        // Reconcile message topics with retry logic
        let _cloud_topics = retry_cloud_operation(|| async {
            cloud_clients.messaging().list_topics().await.map_err(|e| {
                crate::error::RustezeError::cloud(format!("Failed to list topics: {}", e))
            })
        })
        .await
        .map_err(|e| StateError::ReconciliationFailed(e.to_string()))?;

        current_state.message_topics = _cloud_topics;

        // Reconcile message queues
        let _cloud_queues = cloud_clients.messaging().list_queues().await.map_err(|e| {
            StateError::ReconciliationFailed(format!("Failed to list queues: {}", e))
        })?;

        current_state.message_queues = _cloud_queues;

        // Update metadata
        current_state.metadata.last_deployment = chrono::Utc::now();
        current_state.metadata.deployment_id =
            format!("reconcile-{}", chrono::Utc::now().timestamp());

        // Save reconciled state
        self.save_state(&current_state).await?;

        Ok(current_state)
    }

    async fn backup_state(&self) -> Result<String, StateError> {
        if !self.state_file_path.exists() {
            return Err(StateError::FileNotFound);
        }

        self.create_backup()
            .await?
            .ok_or_else(|| StateError::BackupFailed("No state file to backup".to_string()))
    }

    async fn restore_state(&self, backup_path: &str) -> Result<DeploymentState, StateError> {
        let backup_path = Path::new(backup_path);

        if !backup_path.exists() {
            return Err(StateError::RestoreFailed(format!(
                "Backup file not found: {}",
                backup_path.display()
            )));
        }

        let content = fs::read_to_string(backup_path)
            .await
            .map_err(|e| StateError::RestoreFailed(format!("Failed to read backup file: {}", e)))?;

        let state_file = StateFile::from_toml(&content).map_err(|e| {
            StateError::RestoreFailed(format!("Failed to parse backup file: {}", e))
        })?;

        // Validate the backup state
        state_file
            .validate()
            .map_err(|e| StateError::RestoreFailed(format!("Backup validation failed: {}", e)))?;

        // Migrate if necessary
        let migrated_state_file = state_file
            .migrate()
            .map_err(|e| StateError::RestoreFailed(format!("Backup migration failed: {}", e)))?;

        // Save restored state
        self.save_state(&migrated_state_file.state).await?;

        Ok(migrated_state_file.state)
    }

    async fn detect_drift(
        &self,
        cloud_clients: &dyn CloudClients,
    ) -> Result<DriftDetectionResult, StateError> {
        let current_state = self.load_state().await?;

        // If no state exists, everything in cloud is unexpected
        let state = match current_state {
            Some(state) => state,
            None => {
                // No state file exists, so all cloud resources are unexpected
                let cloud_functions =
                    cloud_clients
                        .serverless()
                        .list_functions()
                        .await
                        .map_err(|e| {
                            StateError::ReconciliationFailed(format!(
                                "Failed to list functions: {}",
                                e
                            ))
                        })?;

                let cloud_topics = cloud_clients.messaging().list_topics().await.map_err(|e| {
                    StateError::ReconciliationFailed(format!("Failed to list topics: {}", e))
                })?;

                let cloud_queues = cloud_clients.messaging().list_queues().await.map_err(|e| {
                    StateError::ReconciliationFailed(format!("Failed to list queues: {}", e))
                })?;

                let function_drifts = cloud_functions
                    .into_iter()
                    .map(|f| FunctionDrift {
                        function_name: f.name.clone(),
                        drift_type: ResourceDriftType::Unexpected,
                        expected: None,
                        actual: Some(f),
                        differences: vec!["Resource exists in cloud but not in state".to_string()],
                    })
                    .collect();

                let topic_drifts = cloud_topics
                    .into_iter()
                    .map(|t| TopicDrift {
                        topic_name: t.name.clone(),
                        drift_type: ResourceDriftType::Unexpected,
                        expected: None,
                        actual: Some(t),
                        differences: vec!["Resource exists in cloud but not in state".to_string()],
                    })
                    .collect();

                let queue_drifts = cloud_queues
                    .into_iter()
                    .map(|q| QueueDrift {
                        queue_name: q.name.clone(),
                        drift_type: ResourceDriftType::Unexpected,
                        expected: None,
                        actual: Some(q),
                        differences: vec!["Resource exists in cloud but not in state".to_string()],
                    })
                    .collect();

                return Ok(DriftDetectionResult {
                    has_drift: true,
                    function_drifts,
                    topic_drifts,
                    queue_drifts,
                    api_gateway_drift: None,
                });
            }
        };

        // Get current cloud resources
        let cloud_functions = cloud_clients
            .serverless()
            .list_functions()
            .await
            .map_err(|e| {
                StateError::ReconciliationFailed(format!("Failed to list functions: {}", e))
            })?;

        let cloud_topics = cloud_clients.messaging().list_topics().await.map_err(|e| {
            StateError::ReconciliationFailed(format!("Failed to list topics: {}", e))
        })?;

        let cloud_queues = cloud_clients.messaging().list_queues().await.map_err(|e| {
            StateError::ReconciliationFailed(format!("Failed to list queues: {}", e))
        })?;

        // Detect function drifts
        let mut function_drifts = Vec::new();
        let mut has_drift = false;

        // Check for missing or modified functions
        for expected_function in &state.serverless_functions {
            match cloud_functions
                .iter()
                .find(|f| f.name == expected_function.name)
            {
                Some(actual_function) => {
                    let differences = Self::compare_functions(expected_function, actual_function);
                    if differences.is_empty() {
                        function_drifts.push(FunctionDrift {
                            function_name: expected_function.name.clone(),
                            drift_type: ResourceDriftType::InSync,
                            expected: Some(expected_function.clone()),
                            actual: Some(actual_function.clone()),
                            differences: Vec::new(),
                        });
                    } else {
                        has_drift = true;
                        function_drifts.push(FunctionDrift {
                            function_name: expected_function.name.clone(),
                            drift_type: ResourceDriftType::Modified,
                            expected: Some(expected_function.clone()),
                            actual: Some(actual_function.clone()),
                            differences,
                        });
                    }
                }
                None => {
                    has_drift = true;
                    function_drifts.push(FunctionDrift {
                        function_name: expected_function.name.clone(),
                        drift_type: ResourceDriftType::Missing,
                        expected: Some(expected_function.clone()),
                        actual: None,
                        differences: vec!["Resource exists in state but not in cloud".to_string()],
                    });
                }
            }
        }

        // Check for unexpected functions
        for cloud_function in &cloud_functions {
            if !state
                .serverless_functions
                .iter()
                .any(|f| f.name == cloud_function.name)
            {
                has_drift = true;
                function_drifts.push(FunctionDrift {
                    function_name: cloud_function.name.clone(),
                    drift_type: ResourceDriftType::Unexpected,
                    expected: None,
                    actual: Some(cloud_function.clone()),
                    differences: vec!["Resource exists in cloud but not in state".to_string()],
                });
            }
        }

        // Detect topic drifts (simplified implementation)
        let topic_drifts = Vec::new(); // Placeholder for brevity
        let queue_drifts = Vec::new(); // Placeholder for brevity

        Ok(DriftDetectionResult {
            has_drift,
            function_drifts,
            topic_drifts,
            queue_drifts,
            api_gateway_drift: None,
        })
    }

    async fn reconcile_with_options(
        &self,
        cloud_clients: &dyn CloudClients,
        options: &ReconciliationOptions,
    ) -> Result<DeploymentState, StateError> {
        // First detect drift to understand what needs to be reconciled
        let _drift_result = self.detect_drift(cloud_clients).await?;

        if options.dry_run {
            // For dry run, just return the current state without making changes
            return self.load_state().await?.ok_or_else(|| {
                StateError::ReconciliationFailed("No state file exists for dry run".to_string())
            });
        }

        // For now, delegate to the basic reconcile_with_cloud method
        // In a full implementation, this would handle the different conflict resolution strategies
        self.reconcile_with_cloud(cloud_clients).await
    }
}
