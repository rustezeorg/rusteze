use super::*;
use crate::models::*;
use chrono::Utc;
use std::collections::HashMap;
use tempfile::TempDir;
use tokio::fs;

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_deployment_state() -> DeploymentState {
        DeploymentState {
            metadata: StateMetadata {
                service_name: "test-service".to_string(),
                last_deployment: Utc::now(),
                deployment_id: "dep-123".to_string(),
                provider: "aws".to_string(),
                region: "us-east-1".to_string(),
                last_config_hash: None,
                last_environment: None,
            },
            api_gateway: Some(ApiGatewayState {
                api_id: "abc123".to_string(),
                api_url: "https://abc123.execute-api.us-east-1.amazonaws.com".to_string(),
                stage: "prod".to_string(),
                resource_id: "arn:aws:apigateway:us-east-1::/restapis/abc123".to_string(),
            }),
            serverless_functions: vec![ServerlessFunctionState {
                name: "test-function".to_string(),
                resource_id: "arn:aws:lambda:us-east-1:123456789:function:test-function"
                    .to_string(),
                runtime: "provided.al2".to_string(),
                memory_size: 256,
                timeout: Some(30),
                environment_variables: HashMap::new(),
            }],
            message_topics: vec![MessageTopicState {
                name: "test-topic".to_string(),
                resource_id: "arn:aws:sns:us-east-1:123456789:test-topic".to_string(),
                display_name: Some("Test Topic".to_string()),
                dead_letter_queue_arn: None,
            }],
            message_queues: vec![MessageQueueState {
                name: "test-queue".to_string(),
                resource_id: "https://sqs.us-east-1.amazonaws.com/123456789/test-queue".to_string(),
                topic_subscription: Some("test-topic".to_string()),
                visibility_timeout_seconds: Some(30),
                message_retention_period: Some(1209600),
                dead_letter_queue: None,
            }],
            access_roles: AccessRoleState {
                function_execution_role: "arn:aws:iam::123456789:role/test-role".to_string(),
                additional_policies: vec![],
            },
        }
    }

    #[tokio::test]
    async fn test_file_state_manager_save_and_load() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let state_file_path = temp_dir.path().join("test-state.toml");
        let state_manager = FileStateManager::new(&state_file_path);

        let test_state = create_test_deployment_state();

        // Initially, no state should exist
        let loaded_state = state_manager
            .load_state()
            .await
            .expect("Failed to load state");
        assert!(loaded_state.is_none());

        // Save state
        state_manager
            .save_state(&test_state)
            .await
            .expect("Failed to save state");

        // Verify file was created
        assert!(state_file_path.exists());

        // Load state and verify it matches
        let loaded_state = state_manager
            .load_state()
            .await
            .expect("Failed to load state")
            .expect("State should exist");

        assert_eq!(
            loaded_state.metadata.service_name,
            test_state.metadata.service_name
        );
        assert_eq!(loaded_state.serverless_functions.len(), 1);
        assert_eq!(loaded_state.message_topics.len(), 1);
        assert_eq!(loaded_state.message_queues.len(), 1);
    }

    #[tokio::test]
    async fn test_atomic_write_operation() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let state_file_path = temp_dir.path().join("atomic-test.toml");
        let state_manager = FileStateManager::new(&state_file_path);

        let test_state = create_test_deployment_state();

        // Save initial state
        state_manager
            .save_state(&test_state)
            .await
            .expect("Failed to save initial state");

        // Verify no temporary files remain
        let temp_file_path = state_file_path.with_extension("tmp");
        assert!(
            !temp_file_path.exists(),
            "Temporary file should not exist after atomic write"
        );

        // Verify state file exists and is valid
        assert!(state_file_path.exists());
        let loaded_state = state_manager
            .load_state()
            .await
            .expect("Failed to load state after atomic write");
        assert!(loaded_state.is_some());
    }

    #[tokio::test]
    async fn test_backup_creation() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let state_file_path = temp_dir.path().join("backup-test.toml");
        let state_manager = FileStateManager::new(&state_file_path);

        let test_state = create_test_deployment_state();

        // Save initial state
        state_manager
            .save_state(&test_state)
            .await
            .expect("Failed to save initial state");

        // Create backup
        let backup_path = state_manager
            .backup_state()
            .await
            .expect("Failed to create backup");

        // Verify backup file exists
        assert!(std::path::Path::new(&backup_path).exists());

        // Verify backup directory was created
        let backup_dir = temp_dir.path().join(".rusteze-backups");
        assert!(backup_dir.exists());
        assert!(backup_dir.is_dir());
    }

    #[tokio::test]
    async fn test_backup_and_restore() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let state_file_path = temp_dir.path().join("restore-test.toml");
        let state_manager = FileStateManager::new(&state_file_path);

        let original_state = create_test_deployment_state();

        // Save initial state
        state_manager
            .save_state(&original_state)
            .await
            .expect("Failed to save initial state");

        // Create backup
        let backup_path = state_manager
            .backup_state()
            .await
            .expect("Failed to create backup");

        // Modify the state
        let mut modified_state = original_state.clone();
        modified_state.metadata.service_name = "modified-service".to_string();
        state_manager
            .save_state(&modified_state)
            .await
            .expect("Failed to save modified state");

        // Verify state was modified
        let loaded_modified = state_manager
            .load_state()
            .await
            .expect("Failed to load modified state")
            .expect("Modified state should exist");
        assert_eq!(loaded_modified.metadata.service_name, "modified-service");

        // Restore from backup
        let restored_state = state_manager
            .restore_state(&backup_path)
            .await
            .expect("Failed to restore state");

        // Verify restored state matches original
        assert_eq!(
            restored_state.metadata.service_name,
            original_state.metadata.service_name
        );

        // Verify current state file now contains restored state
        let current_state = state_manager
            .load_state()
            .await
            .expect("Failed to load current state")
            .expect("Current state should exist");
        assert_eq!(
            current_state.metadata.service_name,
            original_state.metadata.service_name
        );
    }

    #[tokio::test]
    async fn test_backup_on_save() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let state_file_path = temp_dir.path().join("backup-on-save-test.toml");
        let state_manager = FileStateManager::new(&state_file_path);

        let initial_state = create_test_deployment_state();

        // Save initial state
        state_manager
            .save_state(&initial_state)
            .await
            .expect("Failed to save initial state");

        // Save a second state (should create backup of first)
        let mut second_state = initial_state.clone();
        second_state.metadata.service_name = "second-service".to_string();
        state_manager
            .save_state(&second_state)
            .await
            .expect("Failed to save second state");

        // Verify backup directory exists and contains backup
        let backup_dir = temp_dir.path().join(".rusteze-backups");
        assert!(backup_dir.exists());

        let mut backup_files = Vec::new();
        let mut read_dir = fs::read_dir(&backup_dir)
            .await
            .expect("Failed to read backup directory");

        while let Some(entry) = read_dir
            .next_entry()
            .await
            .expect("Failed to read directory entry")
        {
            backup_files.push(entry);
        }

        assert!(!backup_files.is_empty(), "Backup should have been created");
    }

    #[tokio::test]
    async fn test_load_nonexistent_state() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let state_file_path = temp_dir.path().join("nonexistent.toml");
        let state_manager = FileStateManager::new(&state_file_path);

        let loaded_state = state_manager
            .load_state()
            .await
            .expect("Loading nonexistent state should not error");

        assert!(loaded_state.is_none());
    }

    #[tokio::test]
    async fn test_load_corrupted_state() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let state_file_path = temp_dir.path().join("corrupted.toml");
        let state_manager = FileStateManager::new(&state_file_path);

        // Write invalid TOML content
        fs::write(&state_file_path, "invalid toml content [[[")
            .await
            .expect("Failed to write corrupted file");

        let result = state_manager.load_state().await;
        assert!(matches!(result, Err(StateError::Corrupted(_))));
    }

    #[tokio::test]
    async fn test_backup_nonexistent_state() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let state_file_path = temp_dir.path().join("nonexistent-backup.toml");
        let state_manager = FileStateManager::new(&state_file_path);

        let result = state_manager.backup_state().await;
        assert!(matches!(result, Err(StateError::FileNotFound)));
    }

    #[tokio::test]
    async fn test_restore_nonexistent_backup() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let state_file_path = temp_dir.path().join("restore-nonexistent.toml");
        let state_manager = FileStateManager::new(&state_file_path);

        let nonexistent_backup = temp_dir.path().join("nonexistent-backup.toml");
        let result = state_manager
            .restore_state(&nonexistent_backup.to_string_lossy())
            .await;

        assert!(matches!(result, Err(StateError::RestoreFailed(_))));
    }

    // Mock cloud clients for testing reconciliation
    struct MockCloudClients;

    #[async_trait]
    impl CloudClients for MockCloudClients {
        fn serverless(&self) -> &dyn ServerlessClient {
            &MockServerlessClient
        }

        fn api_gateway(&self) -> &dyn ApiGatewayClient {
            &MockApiGatewayClient
        }

        fn messaging(&self) -> &dyn MessagingClient {
            &MockMessagingClient
        }

        fn access_control(&self) -> &dyn AccessControlClient {
            &MockAccessControlClient
        }
    }

    struct MockServerlessClient;

    #[async_trait]
    impl ServerlessClient for MockServerlessClient {
        async fn create_function(
            &self,
            _plan: &ServerlessFunctionPlan,
        ) -> Result<ServerlessFunctionState, CloudError> {
            unimplemented!()
        }

        async fn update_function(
            &self,
            _name: &str,
            _plan: &ServerlessFunctionPlan,
        ) -> Result<ServerlessFunctionState, CloudError> {
            unimplemented!()
        }

        async fn delete_function(&self, _name: &str) -> Result<(), CloudError> {
            unimplemented!()
        }

        async fn get_function(
            &self,
            _name: &str,
        ) -> Result<Option<ServerlessFunctionState>, CloudError> {
            unimplemented!()
        }

        async fn list_functions(&self) -> Result<Vec<ServerlessFunctionState>, CloudError> {
            Ok(vec![ServerlessFunctionState {
                name: "cloud-function".to_string(),
                resource_id: "arn:aws:lambda:us-east-1:123456789:function:cloud-function"
                    .to_string(),
                runtime: "provided.al2".to_string(),
                memory_size: 512,
                timeout: Some(60),
                environment_variables: HashMap::new(),
            }])
        }

        async fn update_function_configuration(
            &self,
            _name: &str,
            _config: &FunctionConfiguration,
        ) -> Result<(), CloudError> {
            unimplemented!()
        }
    }

    struct MockApiGatewayClient;

    #[async_trait]
    impl ApiGatewayClient for MockApiGatewayClient {
        async fn create_api(&self, _plan: &ApiGatewayPlan) -> Result<ApiGatewayState, CloudError> {
            unimplemented!()
        }

        async fn update_api(
            &self,
            _api_id: &str,
            _plan: &ApiGatewayPlan,
        ) -> Result<ApiGatewayState, CloudError> {
            unimplemented!()
        }

        async fn delete_api(&self, _api_id: &str) -> Result<(), CloudError> {
            unimplemented!()
        }

        async fn get_api(&self, _api_id: &str) -> Result<Option<ApiGatewayState>, CloudError> {
            unimplemented!()
        }

        async fn deploy_api(&self, _api_id: &str, _stage: &str) -> Result<String, CloudError> {
            unimplemented!()
        }

        async fn create_route(
            &self,
            _api_id: &str,
            _route: &ApiRoutePlan,
        ) -> Result<(), CloudError> {
            unimplemented!()
        }

        async fn delete_route(
            &self,
            _api_id: &str,
            _method: &str,
            _path: &str,
        ) -> Result<(), CloudError> {
            unimplemented!()
        }
    }

    struct MockMessagingClient;

    #[async_trait]
    impl MessagingClient for MockMessagingClient {
        async fn create_topic(
            &self,
            _plan: &MessageTopicPlan,
        ) -> Result<MessageTopicState, CloudError> {
            unimplemented!()
        }

        async fn delete_topic(&self, _topic_id: &str) -> Result<(), CloudError> {
            unimplemented!()
        }

        async fn get_topic(
            &self,
            _topic_id: &str,
        ) -> Result<Option<MessageTopicState>, CloudError> {
            unimplemented!()
        }

        async fn list_topics(&self) -> Result<Vec<MessageTopicState>, CloudError> {
            Ok(vec![MessageTopicState {
                name: "cloud-topic".to_string(),
                resource_id: "arn:aws:sns:us-east-1:123456789:cloud-topic".to_string(),
                display_name: Some("Cloud Topic".to_string()),
                dead_letter_queue_arn: None,
            }])
        }

        async fn create_queue(
            &self,
            _plan: &MessageQueuePlan,
        ) -> Result<MessageQueueState, CloudError> {
            unimplemented!()
        }

        async fn delete_queue(&self, _queue_id: &str) -> Result<(), CloudError> {
            unimplemented!()
        }

        async fn get_queue(
            &self,
            _queue_id: &str,
        ) -> Result<Option<MessageQueueState>, CloudError> {
            unimplemented!()
        }

        async fn list_queues(&self) -> Result<Vec<MessageQueueState>, CloudError> {
            Ok(vec![MessageQueueState {
                name: "cloud-queue".to_string(),
                resource_id: "https://sqs.us-east-1.amazonaws.com/123456789/cloud-queue"
                    .to_string(),
                topic_subscription: Some("cloud-topic".to_string()),
                visibility_timeout_seconds: Some(60),
                message_retention_period: Some(604800),
                dead_letter_queue: None,
            }])
        }

        async fn subscribe_queue_to_topic(
            &self,
            _queue_id: &str,
            _topic_id: &str,
        ) -> Result<String, CloudError> {
            unimplemented!()
        }

        async fn unsubscribe_queue_from_topic(
            &self,
            _subscription_id: &str,
        ) -> Result<(), CloudError> {
            unimplemented!()
        }

        async fn publish_message(
            &self,
            _topic_id: &str,
            _message: &str,
            _attributes: Option<HashMap<String, String>>,
        ) -> Result<String, CloudError> {
            unimplemented!()
        }
    }

    struct MockAccessControlClient;

    #[async_trait]
    impl AccessControlClient for MockAccessControlClient {
        async fn create_execution_role(
            &self,
            _service_name: &str,
            _policies: &[AccessPolicyPlan],
        ) -> Result<String, CloudError> {
            unimplemented!()
        }

        async fn update_execution_role(
            &self,
            _role_arn: &str,
            _policies: &[AccessPolicyPlan],
        ) -> Result<(), CloudError> {
            unimplemented!()
        }

        async fn delete_execution_role(&self, _role_arn: &str) -> Result<(), CloudError> {
            unimplemented!()
        }

        async fn get_execution_role(
            &self,
            _role_arn: &str,
        ) -> Result<Option<AccessRoleState>, CloudError> {
            unimplemented!()
        }

        async fn create_policy(&self, _plan: &AccessPolicyPlan) -> Result<String, CloudError> {
            unimplemented!()
        }

        async fn delete_policy(&self, _policy_arn: &str) -> Result<(), CloudError> {
            unimplemented!()
        }

        async fn attach_policy_to_role(
            &self,
            _role_arn: &str,
            _policy_arn: &str,
        ) -> Result<(), CloudError> {
            unimplemented!()
        }

        async fn detach_policy_from_role(
            &self,
            _role_arn: &str,
            _policy_arn: &str,
        ) -> Result<(), CloudError> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn test_reconcile_with_cloud() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let state_file_path = temp_dir.path().join("reconcile-test.toml");
        let state_manager = FileStateManager::new(&state_file_path);

        let mock_clients = MockCloudClients;

        // Reconcile with cloud (no existing state)
        let reconciled_state = state_manager
            .reconcile_with_cloud(&mock_clients)
            .await
            .expect("Failed to reconcile with cloud");

        // Verify reconciled state contains cloud resources
        assert_eq!(reconciled_state.serverless_functions.len(), 1);
        assert_eq!(
            reconciled_state.serverless_functions[0].name,
            "cloud-function"
        );
        assert_eq!(reconciled_state.message_topics.len(), 1);
        assert_eq!(reconciled_state.message_topics[0].name, "cloud-topic");
        assert_eq!(reconciled_state.message_queues.len(), 1);
        assert_eq!(reconciled_state.message_queues[0].name, "cloud-queue");

        // Verify state was saved
        let loaded_state = state_manager
            .load_state()
            .await
            .expect("Failed to load reconciled state")
            .expect("Reconciled state should exist");

        assert_eq!(loaded_state.serverless_functions.len(), 1);
        assert_eq!(loaded_state.message_topics.len(), 1);
        assert_eq!(loaded_state.message_queues.len(), 1);
    }

    #[tokio::test]
    async fn test_detect_drift_no_state_file() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let state_file_path = temp_dir.path().join("drift-no-state.toml");
        let state_manager = FileStateManager::new(&state_file_path);

        let mock_clients = MockCloudClients;

        // Detect drift when no state file exists
        let drift_result = state_manager
            .detect_drift(&mock_clients)
            .await
            .expect("Failed to detect drift");

        // Should detect drift since cloud resources exist but no state file
        assert!(drift_result.has_drift);
        assert_eq!(drift_result.function_drifts.len(), 1);
        assert_eq!(drift_result.topic_drifts.len(), 1);
        assert_eq!(drift_result.queue_drifts.len(), 1);

        // All resources should be marked as unexpected
        assert!(matches!(
            drift_result.function_drifts[0].drift_type,
            ResourceDriftType::Unexpected
        ));
        assert!(matches!(
            drift_result.topic_drifts[0].drift_type,
            ResourceDriftType::Unexpected
        ));
        assert!(matches!(
            drift_result.queue_drifts[0].drift_type,
            ResourceDriftType::Unexpected
        ));
    }

    #[tokio::test]
    async fn test_detect_drift_with_state() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let state_file_path = temp_dir.path().join("drift-with-state.toml");
        let state_manager = FileStateManager::new(&state_file_path);

        // Create initial state that matches cloud resources
        let initial_state = DeploymentState {
            metadata: StateMetadata {
                service_name: "test-service".to_string(),
                last_deployment: Utc::now(),
                deployment_id: "dep-123".to_string(),
                provider: "aws".to_string(),
                region: "us-east-1".to_string(),
                last_config_hash: None,
                last_environment: None,
            },
            api_gateway: None,
            serverless_functions: vec![ServerlessFunctionState {
                name: "cloud-function".to_string(),
                resource_id: "arn:aws:lambda:us-east-1:123456789:function:cloud-function"
                    .to_string(),
                runtime: "provided.al2".to_string(),
                memory_size: 512,
                timeout: Some(60),
                environment_variables: HashMap::new(),
            }],
            message_topics: vec![MessageTopicState {
                name: "cloud-topic".to_string(),
                resource_id: "arn:aws:sns:us-east-1:123456789:cloud-topic".to_string(),
                display_name: Some("Cloud Topic".to_string()),
                dead_letter_queue_arn: None,
            }],
            message_queues: vec![MessageQueueState {
                name: "cloud-queue".to_string(),
                resource_id: "https://sqs.us-east-1.amazonaws.com/123456789/cloud-queue"
                    .to_string(),
                topic_subscription: Some("cloud-topic".to_string()),
                visibility_timeout_seconds: Some(60),
                message_retention_period: Some(604800),
                dead_letter_queue: None,
            }],
            access_roles: AccessRoleState {
                function_execution_role: "arn:aws:iam::123456789:role/test-role".to_string(),
                additional_policies: vec![],
            },
        };

        state_manager
            .save_state(&initial_state)
            .await
            .expect("Failed to save initial state");

        let mock_clients = MockCloudClients;

        // Detect drift - should show resources are in sync
        let drift_result = state_manager
            .detect_drift(&mock_clients)
            .await
            .expect("Failed to detect drift");

        // Should not detect drift since state matches cloud
        assert!(!drift_result.has_drift);
        assert_eq!(drift_result.function_drifts.len(), 1);
        assert!(matches!(
            drift_result.function_drifts[0].drift_type,
            ResourceDriftType::InSync
        ));
    }

    #[tokio::test]
    async fn test_reconcile_with_options_dry_run() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let state_file_path = temp_dir.path().join("reconcile-dry-run.toml");
        let state_manager = FileStateManager::new(&state_file_path);

        let initial_state = create_test_deployment_state();
        state_manager
            .save_state(&initial_state)
            .await
            .expect("Failed to save initial state");

        let mock_clients = MockCloudClients;
        let options = ReconciliationOptions {
            dry_run: true,
            ..Default::default()
        };

        // Reconcile with dry run option
        let result_state = state_manager
            .reconcile_with_options(&mock_clients, &options)
            .await
            .expect("Failed to reconcile with dry run");

        // State should remain unchanged in dry run
        assert_eq!(
            result_state.metadata.service_name,
            initial_state.metadata.service_name
        );
        assert_eq!(
            result_state.serverless_functions.len(),
            initial_state.serverless_functions.len()
        );
    }

    #[tokio::test]
    async fn test_reconcile_with_options_prefer_cloud() {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let state_file_path = temp_dir.path().join("reconcile-prefer-cloud.toml");
        let state_manager = FileStateManager::new(&state_file_path);

        let initial_state = create_test_deployment_state();
        state_manager
            .save_state(&initial_state)
            .await
            .expect("Failed to save initial state");

        let mock_clients = MockCloudClients;
        let options = ReconciliationOptions {
            conflict_resolution: ConflictResolutionStrategy::PreferCloud,
            dry_run: false,
            ..Default::default()
        };

        // Reconcile with prefer cloud option
        let result_state = state_manager
            .reconcile_with_options(&mock_clients, &options)
            .await
            .expect("Failed to reconcile with prefer cloud");

        // State should be updated to match cloud resources
        assert_eq!(result_state.serverless_functions.len(), 1);
        assert_eq!(result_state.serverless_functions[0].name, "cloud-function");
        assert_eq!(result_state.message_topics.len(), 1);
        assert_eq!(result_state.message_topics[0].name, "cloud-topic");
    }

    #[tokio::test]
    async fn test_function_comparison() {
        let function1 = ServerlessFunctionState {
            name: "test-function".to_string(),
            resource_id: "arn:aws:lambda:us-east-1:123456789:function:test-function".to_string(),
            runtime: "provided.al2".to_string(),
            memory_size: 256,
            timeout: Some(30),
            environment_variables: HashMap::new(),
        };

        let mut function2 = function1.clone();
        function2.memory_size = 512;
        function2.timeout = Some(60);

        let differences = FileStateManager::compare_functions(&function1, &function2);

        assert_eq!(differences.len(), 2);
        assert!(differences.iter().any(|d| d.contains("Memory size")));
        assert!(differences.iter().any(|d| d.contains("Timeout")));
    }

    #[tokio::test]
    async fn test_topic_comparison() {
        let topic1 = MessageTopicState {
            name: "test-topic".to_string(),
            resource_id: "arn:aws:sns:us-east-1:123456789:test-topic".to_string(),
            display_name: Some("Test Topic".to_string()),
            dead_letter_queue_arn: None,
        };

        let mut topic2 = topic1.clone();
        topic2.display_name = Some("Modified Topic".to_string());

        let differences = FileStateManager::compare_topics(&topic1, &topic2);

        assert_eq!(differences.len(), 1);
        assert!(differences[0].contains("Display name"));
    }

    #[tokio::test]
    async fn test_queue_comparison() {
        let queue1 = MessageQueueState {
            name: "test-queue".to_string(),
            resource_id: "https://sqs.us-east-1.amazonaws.com/123456789/test-queue".to_string(),
            topic_subscription: Some("test-topic".to_string()),
            visibility_timeout_seconds: Some(30),
            message_retention_period: Some(1209600),
            dead_letter_queue: None,
        };

        let mut queue2 = queue1.clone();
        queue2.visibility_timeout_seconds = Some(60);
        queue2.message_retention_period = Some(604800);

        let differences = FileStateManager::compare_queues(&queue1, &queue2);

        assert_eq!(differences.len(), 2);
        assert!(differences.iter().any(|d| d.contains("Visibility timeout")));
        assert!(
            differences
                .iter()
                .any(|d| d.contains("Message retention period"))
        );
    }
}
