use crate::cloud::*;
use crate::logging::*;
use crate::models::*;
use crate::retry::*;
use std::path::PathBuf;
use std::process::Command;
use tokio::fs;

/// Deployment engine for managing cloud resource provisioning
pub struct DeploymentEngine {
    cloud_clients: Box<dyn CloudClients>,
    state_manager: Box<dyn StateManager>,
    resource_planner: Box<dyn ResourcePlanner>,
    build_config: BuildConfiguration,
    logger: DeploymentLogger,
}

/// Configuration for building and packaging functions
#[derive(Debug, Clone)]
pub struct BuildConfiguration {
    pub target_dir: PathBuf,
    pub cargo_target: String,
    pub build_profile: String,
    pub lambda_runtime: String,
    pub package_format: PackageFormat,
}

#[derive(Debug, Clone)]
pub enum PackageFormat {
    Zip,
    Container,
}

impl Default for BuildConfiguration {
    fn default() -> Self {
        Self {
            target_dir: PathBuf::from("target"),
            cargo_target: "x86_64-unknown-linux-musl".to_string(),
            build_profile: "release".to_string(),
            lambda_runtime: "provided.al2".to_string(),
            package_format: PackageFormat::Zip,
        }
    }
}

/// Result of a deployment operation
#[derive(Debug, Clone)]
pub struct DeploymentResult {
    pub deployment_id: String,
    pub deployed_functions: Vec<ServerlessFunctionState>,
    pub api_gateway: Option<ApiGatewayState>,
    pub message_topics: Vec<MessageTopicState>,
    pub message_queues: Vec<MessageQueueState>,
    pub access_roles: AccessRoleState,
    pub deployment_summary: DeploymentSummary,
}

/// Summary of deployment changes
#[derive(Debug, Clone)]
pub struct DeploymentSummary {
    pub functions_created: u32,
    pub functions_updated: u32,
    pub functions_deleted: u32,
    pub topics_created: u32,
    pub queues_created: u32,
    pub api_gateway_created: bool,
    pub api_gateway_updated: bool,
    pub total_duration_seconds: u64,
}

/// Errors that can occur during deployment
#[derive(Debug, thiserror::Error)]
pub enum DeploymentError {
    #[error("Build failed: {0}")]
    BuildFailed(String),

    #[error("Package failed: {0}")]
    PackageFailed(String),

    #[error("Cloud deployment failed: {0}")]
    CloudDeploymentFailed(#[from] CloudError),

    #[error("State management failed: {0}")]
    StateManagementFailed(#[from] StateError),

    #[error("Planning failed: {0}")]
    PlanningFailed(#[from] PlanningError),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),
}

impl From<crate::error::RustezeError> for DeploymentError {
    fn from(err: crate::error::RustezeError) -> Self {
        match err {
            crate::error::RustezeError::Cloud {
                source: Some(cloud_err),
                ..
            } => DeploymentError::CloudDeploymentFailed(cloud_err),
            crate::error::RustezeError::State {
                source: Some(state_err),
                ..
            } => DeploymentError::StateManagementFailed(state_err),
            crate::error::RustezeError::Planning {
                source: Some(planning_err),
                ..
            } => DeploymentError::PlanningFailed(planning_err),
            crate::error::RustezeError::Io { source, .. } => DeploymentError::IoError(source),
            _ => DeploymentError::InvalidConfiguration(err.to_string()),
        }
    }
}

impl DeploymentEngine {
    /// Create a new deployment engine
    pub fn new(
        cloud_clients: Box<dyn CloudClients>,
        state_manager: Box<dyn StateManager>,
        resource_planner: Box<dyn ResourcePlanner>,
    ) -> Result<Self, DeploymentError> {
        let logger = DeploymentLogger::default().map_err(|e| {
            DeploymentError::InvalidConfiguration(format!("Failed to create logger: {}", e))
        })?;

        Ok(Self {
            cloud_clients,
            state_manager,
            resource_planner,
            build_config: BuildConfiguration::default(),
            logger,
        })
    }

    /// Create a new deployment engine with custom build configuration
    pub fn with_build_config(
        cloud_clients: Box<dyn CloudClients>,
        state_manager: Box<dyn StateManager>,
        resource_planner: Box<dyn ResourcePlanner>,
        build_config: BuildConfiguration,
    ) -> Result<Self, DeploymentError> {
        let logger = DeploymentLogger::default().map_err(|e| {
            DeploymentError::InvalidConfiguration(format!("Failed to create logger: {}", e))
        })?;

        Ok(Self {
            cloud_clients,
            state_manager,
            resource_planner,
            build_config,
            logger,
        })
    }

    /// Deploy resources based on route metadata
    pub async fn deploy(
        &mut self,
        metadata: &[RouteMetadata],
    ) -> Result<DeploymentResult, DeploymentError> {
        let start_time = std::time::Instant::now();
        let deployment_id = format!("deploy-{}", chrono::Utc::now().timestamp());

        // Start deployment logging
        self.logger
            .start_deployment(deployment_id.clone())
            .map_err(|e| {
                DeploymentError::InvalidConfiguration(format!("Failed to start logging: {}", e))
            })?;

        // Load current state with timeout
        let current_state = with_timeout(
            || async {
                self.state_manager
                    .load_state()
                    .await
                    .map_err(|e| crate::error::RustezeError::state(e.to_string()))
            },
            std::time::Duration::from_secs(30),
        )
        .await?;

        // Analyze code and create resource plan
        let mut resource_plan =
            self.logger
                .timed("analyze_code", "planning", "resource_plan", || {
                    self.resource_planner
                        .analyze_code(metadata)
                        .map_err(|e| e.into())
                })?;

        // Generate access policies
        resource_plan.access_policies = self.generate_access_policies(&resource_plan);

        // Validate the plan
        self.logger
            .timed("validate_plan", "planning", "resource_plan", || {
                self.resource_planner
                    .validate_plan(&resource_plan)
                    .map_err(|e| e.into())
            })?;

        // Calculate deployment dependencies
        let dependencies = self.logger.timed(
            "calculate_dependencies",
            "planning",
            "resource_plan",
            || {
                self.resource_planner
                    .calculate_dependencies(&resource_plan)
                    .map_err(|e| e.into())
            },
        )?;

        let mut deployment_summary = DeploymentSummary {
            functions_created: 0,
            functions_updated: 0,
            functions_deleted: 0,
            topics_created: 0,
            queues_created: 0,
            api_gateway_created: false,
            api_gateway_updated: false,
            total_duration_seconds: 0,
        };

        // Deploy resources in dependency order
        let mut deployed_functions = Vec::new();
        let mut deployed_topics = Vec::new();
        let mut deployed_queues = Vec::new();
        let mut deployed_api_gateway = None;
        let mut access_roles = AccessRoleState {
            function_execution_role: "".to_string(),
            additional_policies: Vec::new(),
        };

        // Deploy resources in dependency order with logging
        for dependency in dependencies {
            match dependency.as_str() {
                "access_roles" => {
                    let start_time = std::time::Instant::now();
                    self.logger
                        .log_operation(
                            "deploy_access_roles",
                            "access",
                            "execution_role",
                            OperationStatus::Started,
                            None,
                            std::collections::HashMap::new(),
                            None,
                        )
                        .ok();

                    access_roles = self
                        .deploy_access_roles(&resource_plan, &current_state)
                        .await?;

                    let duration_ms = start_time.elapsed().as_millis() as u64;
                    self.logger
                        .log_operation(
                            "deploy_access_roles",
                            "access",
                            "execution_role",
                            OperationStatus::Completed,
                            Some(duration_ms),
                            std::collections::HashMap::new(),
                            None,
                        )
                        .ok();

                    self.logger
                        .log_audit(
                            AuditAction::Create,
                            "access_role",
                            &access_roles.function_execution_role,
                            Some(access_roles.function_execution_role.clone()),
                            None,
                            Some("Created execution role".to_string()),
                            std::collections::HashMap::new(),
                        )
                        .ok();
                }
                "message_topics" => {
                    let start_time = std::time::Instant::now();
                    self.logger
                        .log_operation(
                            "deploy_message_topics",
                            "messaging",
                            "topics",
                            OperationStatus::Started,
                            None,
                            std::collections::HashMap::new(),
                            None,
                        )
                        .ok();

                    let (topics, created_count) = self
                        .deploy_message_topics(&resource_plan, &current_state)
                        .await?;

                    let duration_ms = start_time.elapsed().as_millis() as u64;
                    self.logger
                        .log_operation(
                            "deploy_message_topics",
                            "messaging",
                            "topics",
                            OperationStatus::Completed,
                            Some(duration_ms),
                            std::collections::HashMap::new(),
                            None,
                        )
                        .ok();

                    for topic in &topics {
                        self.logger
                            .log_audit(
                                AuditAction::Create,
                                "message_topic",
                                &topic.name,
                                Some(topic.resource_id.clone()),
                                None,
                                Some("Created message topic".to_string()),
                                std::collections::HashMap::new(),
                            )
                            .ok();

                        self.logger.add_resource_summary(
                            "message_topic",
                            &topic.name,
                            AuditAction::Create,
                            OperationStatus::Completed,
                            Some(duration_ms),
                            None,
                        );
                    }

                    deployed_topics = topics;
                    deployment_summary.topics_created = created_count;
                }
                "message_queues" => {
                    let start_time = std::time::Instant::now();
                    self.logger
                        .log_operation(
                            "deploy_message_queues",
                            "messaging",
                            "queues",
                            OperationStatus::Started,
                            None,
                            std::collections::HashMap::new(),
                            None,
                        )
                        .ok();

                    let (queues, created_count) = self
                        .deploy_message_queues(&resource_plan, &current_state, &deployed_topics)
                        .await?;

                    let duration_ms = start_time.elapsed().as_millis() as u64;
                    self.logger
                        .log_operation(
                            "deploy_message_queues",
                            "messaging",
                            "queues",
                            OperationStatus::Completed,
                            Some(duration_ms),
                            std::collections::HashMap::new(),
                            None,
                        )
                        .ok();

                    for queue in &queues {
                        self.logger
                            .log_audit(
                                AuditAction::Create,
                                "message_queue",
                                &queue.name,
                                Some(queue.resource_id.clone()),
                                None,
                                Some("Created message queue".to_string()),
                                std::collections::HashMap::new(),
                            )
                            .ok();

                        self.logger.add_resource_summary(
                            "message_queue",
                            &queue.name,
                            AuditAction::Create,
                            OperationStatus::Completed,
                            Some(duration_ms),
                            None,
                        );
                    }

                    deployed_queues = queues;
                    deployment_summary.queues_created = created_count;
                }
                "serverless_functions" => {
                    let start_time = std::time::Instant::now();
                    self.logger
                        .log_operation(
                            "deploy_serverless_functions",
                            "serverless",
                            "functions",
                            OperationStatus::Started,
                            None,
                            std::collections::HashMap::new(),
                            None,
                        )
                        .ok();

                    let (functions, created, updated) = self
                        .deploy_serverless_functions(&resource_plan, &current_state)
                        .await?;

                    let duration_ms = start_time.elapsed().as_millis() as u64;
                    self.logger
                        .log_operation(
                            "deploy_serverless_functions",
                            "serverless",
                            "functions",
                            OperationStatus::Completed,
                            Some(duration_ms),
                            std::collections::HashMap::new(),
                            None,
                        )
                        .ok();

                    for function in &functions {
                        let action = if created > 0 {
                            AuditAction::Create
                        } else {
                            AuditAction::Update
                        };
                        self.logger
                            .log_audit(
                                action.clone(),
                                "serverless_function",
                                &function.name,
                                Some(function.resource_id.clone()),
                                None,
                                Some(format!("{} serverless function", action)),
                                std::collections::HashMap::new(),
                            )
                            .ok();

                        self.logger.add_resource_summary(
                            "serverless_function",
                            &function.name,
                            action,
                            OperationStatus::Completed,
                            Some(duration_ms),
                            None,
                        );
                    }

                    deployed_functions = functions;
                    deployment_summary.functions_created = created;
                    deployment_summary.functions_updated = updated;
                }
                "api_gateway" => {
                    if let Some(api_plan) = &resource_plan.api_gateway {
                        let start_time = std::time::Instant::now();
                        self.logger
                            .log_operation(
                                "deploy_api_gateway",
                                "api_gateway",
                                &api_plan.name,
                                OperationStatus::Started,
                                None,
                                std::collections::HashMap::new(),
                                None,
                            )
                            .ok();

                        let (api_state, created, updated) = self
                            .deploy_api_gateway(api_plan, &current_state, &deployed_functions)
                            .await?;

                        let duration_ms = start_time.elapsed().as_millis() as u64;
                        self.logger
                            .log_operation(
                                "deploy_api_gateway",
                                "api_gateway",
                                &api_plan.name,
                                OperationStatus::Completed,
                                Some(duration_ms),
                                std::collections::HashMap::new(),
                                None,
                            )
                            .ok();

                        let action = if created {
                            AuditAction::Create
                        } else {
                            AuditAction::Update
                        };
                        self.logger
                            .log_audit(
                                action.clone(),
                                "api_gateway",
                                &api_plan.name,
                                Some(api_state.api_id.clone()),
                                None,
                                Some(format!("{} API Gateway", action)),
                                std::collections::HashMap::new(),
                            )
                            .ok();

                        self.logger.add_resource_summary(
                            "api_gateway",
                            &api_plan.name,
                            action,
                            OperationStatus::Completed,
                            Some(duration_ms),
                            None,
                        );

                        deployed_api_gateway = Some(api_state);
                        deployment_summary.api_gateway_created = created;
                        deployment_summary.api_gateway_updated = updated;
                    }
                }
                _ => {
                    let error_msg = format!("Unknown dependency: {}", dependency);
                    self.logger.add_error(error_msg.clone());
                    return Err(DeploymentError::InvalidConfiguration(error_msg));
                }
            }
        }

        // Update access roles with final policies now that all resources are deployed
        access_roles = self
            .update_access_roles_with_resources(
                &access_roles,
                &deployed_topics,
                &deployed_queues,
                &deployed_functions,
            )
            .await?;

        // Create new deployment state
        let new_state = DeploymentState {
            metadata: StateMetadata {
                service_name: "rusteze-service".to_string(), // TODO: Get from config
                last_deployment: chrono::Utc::now(),
                deployment_id: deployment_id.clone(),
                provider: "aws".to_string(),
                region: "us-east-1".to_string(), // TODO: Get from config
                last_config_hash: None,
                last_environment: None,
            },
            api_gateway: deployed_api_gateway,
            serverless_functions: deployed_functions.clone(),
            message_topics: deployed_topics,
            message_queues: deployed_queues,
            access_roles,
        };

        // Save the new state with timeout and retry
        with_timeout(
            || async {
                self.state_manager
                    .save_state(&new_state)
                    .await
                    .map_err(|e| crate::error::RustezeError::state(e.to_string()))
            },
            std::time::Duration::from_secs(60),
        )
        .await?;

        deployment_summary.total_duration_seconds = start_time.elapsed().as_secs();

        // End deployment logging
        let final_status = if deployment_summary.functions_created > 0
            || deployment_summary.functions_updated > 0
            || deployment_summary.topics_created > 0
            || deployment_summary.queues_created > 0
            || deployment_summary.api_gateway_created
            || deployment_summary.api_gateway_updated
        {
            DeploymentSummaryStatus::Completed
        } else {
            DeploymentSummaryStatus::PartiallyCompleted
        };

        self.logger.end_deployment(final_status).map_err(|e| {
            DeploymentError::InvalidConfiguration(format!("Failed to end logging: {}", e))
        })?;

        // Print deployment summary
        self.logger.print_summary();

        Ok(DeploymentResult {
            deployment_id,
            deployed_functions,
            api_gateway: new_state.api_gateway,
            message_topics: new_state.message_topics,
            message_queues: new_state.message_queues,
            access_roles: new_state.access_roles,
            deployment_summary,
        })
    }

    /// Build and package a serverless function
    async fn build_and_package_function(
        &self,
        function_plan: &ServerlessFunctionPlan,
    ) -> Result<Vec<u8>, DeploymentError> {
        // Build the function
        self.build_function(function_plan).await?;

        // Package the function
        self.package_function(function_plan).await
    }

    /// Build a serverless function using Cargo
    async fn build_function(
        &self,
        function_plan: &ServerlessFunctionPlan,
    ) -> Result<(), DeploymentError> {
        println!("Building function: {}", function_plan.name);

        let mut build_command = Command::new("cargo");
        build_command
            .arg("build")
            .arg("--target")
            .arg(&self.build_config.cargo_target);

        if self.build_config.build_profile == "release" {
            build_command.arg("--release");
        }

        let output = build_command.output().map_err(|e| {
            DeploymentError::BuildFailed(format!("Failed to execute cargo build: {}", e))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(DeploymentError::BuildFailed(format!(
                "Cargo build failed: {}",
                stderr
            )));
        }

        println!("Successfully built function: {}", function_plan.name);
        Ok(())
    }

    /// Package a serverless function into a deployment artifact
    async fn package_function(
        &self,
        function_plan: &ServerlessFunctionPlan,
    ) -> Result<Vec<u8>, DeploymentError> {
        match self.build_config.package_format {
            PackageFormat::Zip => self.package_function_as_zip(function_plan).await,
            PackageFormat::Container => {
                // Container packaging would be implemented here
                Err(DeploymentError::PackageFailed(
                    "Container packaging not yet implemented".to_string(),
                ))
            }
        }
    }

    /// Package a function as a ZIP file
    async fn package_function_as_zip(
        &self,
        function_plan: &ServerlessFunctionPlan,
    ) -> Result<Vec<u8>, DeploymentError> {
        let binary_path = self
            .build_config
            .target_dir
            .join(&self.build_config.cargo_target)
            .join(&self.build_config.build_profile)
            .join(&function_plan.handler);

        if !binary_path.exists() {
            return Err(DeploymentError::PackageFailed(format!(
                "Binary not found at: {}",
                binary_path.display()
            )));
        }

        // For now, return the binary content directly
        // In a real implementation, this would create a proper ZIP file
        let binary_content = fs::read(&binary_path)
            .await
            .map_err(|e| DeploymentError::PackageFailed(format!("Failed to read binary: {}", e)))?;

        // Create a simple ZIP-like structure (placeholder implementation)
        // In production, you'd use a proper ZIP library
        Ok(binary_content)
    }

    /// Deploy serverless functions
    async fn deploy_serverless_functions(
        &mut self,
        resource_plan: &ResourcePlan,
        current_state: &Option<DeploymentState>,
    ) -> Result<(Vec<ServerlessFunctionState>, u32, u32), DeploymentError> {
        let mut deployed_functions = Vec::new();
        let mut created_count = 0;
        let mut updated_count = 0;

        let empty_functions = Vec::new();
        let current_functions = current_state
            .as_ref()
            .map(|s| &s.serverless_functions)
            .unwrap_or(&empty_functions);

        for function_plan in &resource_plan.serverless_functions {
            // Check if function already exists
            let existing_function = current_functions
                .iter()
                .find(|f| f.name == function_plan.name);

            // Build and package the function
            let _package_data = self.build_and_package_function(function_plan).await?;

            let function_state = match existing_function {
                Some(_) => {
                    // Update existing function
                    println!("Updating function: {}", function_plan.name);
                    let updated_function = self
                        .cloud_clients
                        .serverless()
                        .update_function(&function_plan.name, function_plan)
                        .await?;
                    updated_count += 1;
                    updated_function
                }
                None => {
                    // Create new function
                    println!("Creating function: {}", function_plan.name);
                    let new_function = self
                        .cloud_clients
                        .serverless()
                        .create_function(function_plan)
                        .await?;
                    created_count += 1;
                    new_function
                }
            };

            deployed_functions.push(function_state);
        }

        Ok((deployed_functions, created_count, updated_count))
    }

    /// Deploy access roles and policies using PolicyManager
    async fn deploy_access_roles(
        &mut self,
        resource_plan: &ResourcePlan,
        current_state: &Option<DeploymentState>,
    ) -> Result<AccessRoleState, DeploymentError> {
        use crate::policy::PolicyManager;

        let service_name = "rusteze-service"; // TODO: Get from config

        // Create PolicyManager for enhanced policy management
        let policy_manager = PolicyManager::new(
            Box::new(crate::aws_mock::MockAwsAccessControlClient::new()), // TODO: Use actual access control client
            service_name.to_string(),
        );

        // Check if execution role already exists
        let existing_role = current_state
            .as_ref()
            .map(|s| &s.access_roles.function_execution_role);

        let access_role_state = match existing_role {
            Some(existing_arn) if !existing_arn.is_empty() => {
                // Update existing role with new policies
                println!("Updating execution role: {}", existing_arn);

                // For now, we'll use empty collections since we don't have the deployed resources yet
                // In a real implementation, this would be called after topics and queues are deployed
                let topics = vec![];
                let queues = vec![];
                let functions = vec![];

                let updated_policies = policy_manager
                    .update_execution_role_policies(existing_arn, &topics, &queues, &functions)
                    .await
                    .map_err(|e| {
                        DeploymentError::CloudDeploymentFailed(CloudError::Unknown(format!(
                            "Policy update failed: {}",
                            e
                        )))
                    })?;

                AccessRoleState {
                    function_execution_role: existing_arn.clone(),
                    additional_policies: updated_policies,
                }
            }
            _ => {
                // Create new role with policies
                println!("Creating execution role for service: {}", service_name);

                // For now, we'll use empty collections since we don't have the deployed resources yet
                // In a real implementation, this would be called after topics and queues are deployed
                let topics = vec![];
                let queues = vec![];
                let functions = vec![];

                policy_manager
                    .create_execution_role_with_policies(&topics, &queues, &functions)
                    .await
                    .map_err(|e| {
                        DeploymentError::CloudDeploymentFailed(CloudError::Unknown(format!(
                            "Role creation failed: {}",
                            e
                        )))
                    })?
            }
        };

        Ok(access_role_state)
    }

    /// Deploy message topics using TopicManager
    async fn deploy_message_topics(
        &mut self,
        resource_plan: &ResourcePlan,
        current_state: &Option<DeploymentState>,
    ) -> Result<(Vec<MessageTopicState>, u32), DeploymentError> {
        use crate::messaging::TopicManager;

        let empty_topics = Vec::new();
        let current_topics = current_state
            .as_ref()
            .map(|s| &s.message_topics)
            .unwrap_or(&empty_topics);

        // Create TopicManager for enhanced topic provisioning
        let topic_manager = TopicManager::new(
            Box::new(crate::aws_mock::MockMessagingClient::new()), // TODO: Use actual messaging client
            "rusteze-service".to_string(),                         // TODO: Get from config
        );

        // Provision topics with dead letter queue support
        let deployed_topics = topic_manager
            .provision_topics(&resource_plan.message_topics, current_topics)
            .await
            .map_err(|e| {
                DeploymentError::CloudDeploymentFailed(CloudError::Unknown(format!(
                    "Topic provisioning failed: {}",
                    e
                )))
            })?;

        let created_count = deployed_topics.len() as u32 - current_topics.len() as u32;

        Ok((deployed_topics, created_count))
    }

    /// Deploy message queues using QueueManager
    async fn deploy_message_queues(
        &mut self,
        resource_plan: &ResourcePlan,
        current_state: &Option<DeploymentState>,
        deployed_topics: &[MessageTopicState],
    ) -> Result<(Vec<MessageQueueState>, u32), DeploymentError> {
        use crate::messaging::QueueManager;

        let empty_queues = Vec::new();
        let current_queues = current_state
            .as_ref()
            .map(|s| &s.message_queues)
            .unwrap_or(&empty_queues);

        // Create QueueManager for enhanced queue provisioning
        let queue_manager = QueueManager::new(
            Box::new(crate::aws_mock::MockMessagingClient::new()), // TODO: Use actual messaging client
            "rusteze-service".to_string(),                         // TODO: Get from config
        );

        // Provision queues with automatic topic subscription and event source mapping
        let deployed_queues = queue_manager
            .provision_queues(
                &resource_plan.message_queues,
                current_queues,
                deployed_topics,
            )
            .await
            .map_err(|e| {
                DeploymentError::CloudDeploymentFailed(CloudError::Unknown(format!(
                    "Queue provisioning failed: {}",
                    e
                )))
            })?;

        let created_count = deployed_queues.len() as u32 - current_queues.len() as u32;

        Ok((deployed_queues, created_count))
    }

    /// Update access roles with final policies after all resources are deployed
    async fn update_access_roles_with_resources(
        &mut self,
        access_roles: &AccessRoleState,
        deployed_topics: &[MessageTopicState],
        deployed_queues: &[MessageQueueState],
        deployed_functions: &[ServerlessFunctionState],
    ) -> Result<AccessRoleState, DeploymentError> {
        use crate::policy::PolicyManager;

        let service_name = "rusteze-service"; // TODO: Get from config

        // Create PolicyManager for final policy updates
        let policy_manager = PolicyManager::new(
            Box::new(crate::aws_mock::MockAwsAccessControlClient::new()), // TODO: Use actual access control client
            service_name.to_string(),
        );

        // Update execution role with comprehensive policies
        let updated_policies = policy_manager
            .update_execution_role_policies(
                &access_roles.function_execution_role,
                deployed_topics,
                deployed_queues,
                deployed_functions,
            )
            .await
            .map_err(|e| {
                DeploymentError::CloudDeploymentFailed(CloudError::Unknown(format!(
                    "Final policy update failed: {}",
                    e
                )))
            })?;

        Ok(AccessRoleState {
            function_execution_role: access_roles.function_execution_role.clone(),
            additional_policies: updated_policies,
        })
    }

    /// Deploy API Gateway
    async fn deploy_api_gateway(
        &mut self,
        api_plan: &ApiGatewayPlan,
        current_state: &Option<DeploymentState>,
        _deployed_functions: &[ServerlessFunctionState],
    ) -> Result<(ApiGatewayState, bool, bool), DeploymentError> {
        let existing_api = current_state.as_ref().and_then(|s| s.api_gateway.as_ref());

        let (api_state, created, updated) = match existing_api {
            Some(existing) => {
                // Update existing API Gateway
                println!("Updating API Gateway: {}", existing.api_id);
                let updated_api = self
                    .cloud_clients
                    .api_gateway()
                    .update_api(&existing.api_id, api_plan)
                    .await?;
                (updated_api, false, true)
            }
            None => {
                // Create new API Gateway
                println!("Creating API Gateway: {}", api_plan.name);
                let new_api = self
                    .cloud_clients
                    .api_gateway()
                    .create_api(api_plan)
                    .await?;
                (new_api, true, false)
            }
        };

        Ok((api_state, created, updated))
    }

    /// Generate access policies for the resource plan
    fn generate_access_policies(&self, resource_plan: &ResourcePlan) -> Vec<AccessPolicyPlan> {
        let mut policies = Vec::new();

        // SNS publish policy if there are topics
        if !resource_plan.message_topics.is_empty() {
            let topic_arns: Vec<String> = resource_plan
                .message_topics
                .iter()
                .map(|t| format!("arn:aws:sns:*:*:{}", t.name))
                .collect();

            let sns_policy = AccessPolicyPlan {
                name: "sns-publish-policy".to_string(),
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
                attached_to: vec!["execution-role".to_string()],
            };
            policies.push(sns_policy);
        }

        // SQS consume policy if there are queues
        if !resource_plan.message_queues.is_empty() {
            let queue_arns: Vec<String> = resource_plan
                .message_queues
                .iter()
                .map(|q| format!("arn:aws:sqs:*:*:{}", q.name))
                .collect();

            let sqs_policy = AccessPolicyPlan {
                name: "sqs-consume-policy".to_string(),
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
                attached_to: vec!["execution-role".to_string()],
            };
            policies.push(sqs_policy);
        }

        policies
    }
}
impl DeploymentEngine {
    /// Deploy API Gateway with enhanced integration setup
    pub async fn deploy_api_gateway_with_integrations(
        &mut self,
        api_plan: &ApiGatewayPlan,
        current_state: &Option<DeploymentState>,
        deployed_functions: &[ServerlessFunctionState],
    ) -> Result<(ApiGatewayState, bool, bool), DeploymentError> {
        let existing_api = current_state.as_ref().and_then(|s| s.api_gateway.as_ref());

        let (mut api_state, created, updated) = match existing_api {
            Some(existing) => {
                // Update existing API Gateway
                println!("Updating API Gateway: {}", existing.api_id);
                let updated_api = self
                    .cloud_clients
                    .api_gateway()
                    .update_api(&existing.api_id, api_plan)
                    .await?;
                (updated_api, false, true)
            }
            None => {
                // Create new API Gateway
                println!("Creating API Gateway: {}", api_plan.name);
                let new_api = self
                    .cloud_clients
                    .api_gateway()
                    .create_api(api_plan)
                    .await?;
                (new_api, true, false)
            }
        };

        // Set up Lambda permissions for API Gateway to invoke functions
        for route in &api_plan.routes {
            if let Some(function) = deployed_functions
                .iter()
                .find(|f| f.name == route.function_name)
            {
                self.setup_lambda_api_gateway_permission(
                    &api_state.api_id,
                    &function.resource_id,
                    route,
                )
                .await?;
            }
        }

        // Update API URL with the correct stage
        api_state.api_url = format!(
            "https://{}.execute-api.us-east-1.amazonaws.com/{}",
            api_state.api_id, api_state.stage
        );

        Ok((api_state, created, updated))
    }

    /// Set up Lambda permission for API Gateway to invoke the function
    async fn setup_lambda_api_gateway_permission(
        &self,
        _api_id: &str,
        function_arn: &str,
        route: &ApiRoutePlan,
    ) -> Result<(), DeploymentError> {
        // In a real implementation, this would use AWS Lambda's add_permission API
        // to allow API Gateway to invoke the Lambda function
        println!(
            "Setting up API Gateway permission for function {} on route {} {}",
            function_arn, route.method, route.path
        );

        // This is a placeholder - in production you would:
        // 1. Extract function name from ARN
        // 2. Call lambda_client.add_permission() with appropriate parameters
        // 3. Handle permission already exists errors gracefully

        Ok(())
    }

    /// Validate API Gateway configuration before deployment
    pub fn validate_api_gateway_plan(
        &self,
        api_plan: &ApiGatewayPlan,
        functions: &[ServerlessFunctionPlan],
    ) -> Result<(), DeploymentError> {
        // Validate API Gateway name
        if api_plan.name.is_empty() {
            return Err(DeploymentError::InvalidConfiguration(
                "API Gateway name cannot be empty".to_string(),
            ));
        }

        // Validate stage name
        if api_plan.stage.is_empty() {
            return Err(DeploymentError::InvalidConfiguration(
                "API Gateway stage cannot be empty".to_string(),
            ));
        }

        // Validate that stage name follows AWS naming conventions
        if !api_plan
            .stage
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err(DeploymentError::InvalidConfiguration(
                "API Gateway stage name can only contain alphanumeric characters, hyphens, and underscores".to_string(),
            ));
        }

        // Validate routes
        let mut route_keys = std::collections::HashSet::new();
        for route in &api_plan.routes {
            // Check for duplicate routes
            let route_key = format!("{} {}", route.method, route.path);
            if !route_keys.insert(route_key.clone()) {
                return Err(DeploymentError::InvalidConfiguration(format!(
                    "Duplicate API route: {}",
                    route_key
                )));
            }

            // Validate HTTP method
            let valid_methods = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];
            if !valid_methods.contains(&route.method.as_str()) {
                return Err(DeploymentError::InvalidConfiguration(format!(
                    "Invalid HTTP method: {}",
                    route.method
                )));
            }

            // Validate path format
            if !route.path.starts_with('/') {
                return Err(DeploymentError::InvalidConfiguration(format!(
                    "API route path must start with '/': {}",
                    route.path
                )));
            }

            // Validate that the referenced function exists
            if !functions.iter().any(|f| f.name == route.function_name) {
                return Err(DeploymentError::InvalidConfiguration(format!(
                    "Function '{}' referenced by route '{}' does not exist",
                    route.function_name, route_key
                )));
            }
        }

        Ok(())
    }

    /// Get API Gateway deployment status
    pub async fn get_api_gateway_status(
        &self,
        api_id: &str,
    ) -> Result<ApiGatewayStatus, DeploymentError> {
        match self.cloud_clients.api_gateway().get_api(api_id).await? {
            Some(api_state) => {
                // Check if API is properly deployed by attempting to get routes
                // This is a simplified check - in production you might want more comprehensive health checks
                Ok(ApiGatewayStatus {
                    api_id: api_state.api_id,
                    status: ApiGatewayDeploymentStatus::Deployed,
                    endpoint_url: Some(api_state.api_url),
                    stage: api_state.stage,
                    last_deployment_time: None, // Would be populated from AWS API
                })
            }
            None => Ok(ApiGatewayStatus {
                api_id: api_id.to_string(),
                status: ApiGatewayDeploymentStatus::NotFound,
                endpoint_url: None,
                stage: "".to_string(),
                last_deployment_time: None,
            }),
        }
    }

    /// Delete API Gateway and clean up resources
    pub async fn delete_api_gateway(&mut self, api_id: &str) -> Result<(), DeploymentError> {
        println!("Deleting API Gateway: {}", api_id);

        // Delete the API Gateway
        self.cloud_clients.api_gateway().delete_api(api_id).await?;

        println!("Successfully deleted API Gateway: {}", api_id);
        Ok(())
    }

    /// Update API Gateway routes without recreating the entire API
    pub async fn update_api_gateway_routes(
        &mut self,
        api_id: &str,
        new_routes: &[ApiRoutePlan],
        deployed_functions: &[ServerlessFunctionState],
    ) -> Result<(), DeploymentError> {
        println!("Updating API Gateway routes for: {}", api_id);

        // Get current routes (this would require additional API calls in real implementation)
        // For now, we'll assume we need to recreate all routes

        // Create new routes
        for route in new_routes {
            println!("Creating/updating route: {} {}", route.method, route.path);

            self.cloud_clients
                .api_gateway()
                .create_route(api_id, route)
                .await?;

            // Set up permissions for the new route
            if let Some(function) = deployed_functions
                .iter()
                .find(|f| f.name == route.function_name)
            {
                self.setup_lambda_api_gateway_permission(api_id, &function.resource_id, route)
                    .await?;
            }
        }

        // Redeploy the API to activate changes
        let deployment_url = self
            .cloud_clients
            .api_gateway()
            .deploy_api(api_id, "prod")
            .await?;

        println!("API Gateway redeployed successfully: {}", deployment_url);
        Ok(())
    }
}

/// Status information for API Gateway deployment
#[derive(Debug, Clone)]
pub struct ApiGatewayStatus {
    pub api_id: String,
    pub status: ApiGatewayDeploymentStatus,
    pub endpoint_url: Option<String>,
    pub stage: String,
    pub last_deployment_time: Option<chrono::DateTime<chrono::Utc>>,
}

/// API Gateway deployment status
#[derive(Debug, Clone, PartialEq)]
pub enum ApiGatewayDeploymentStatus {
    NotFound,
    Creating,
    Deployed,
    Updating,
    Failed,
}
