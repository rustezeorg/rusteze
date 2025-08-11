use super::*;
use chrono::Utc;
use std::collections::HashMap;

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

    #[test]
    fn test_state_file_serialization() {
        let state = create_test_deployment_state();
        let state_file = StateFile::new(state);

        // Test TOML serialization
        let toml_content = state_file.to_toml().expect("Failed to serialize to TOML");
        assert!(!toml_content.is_empty());
        assert!(toml_content.contains("version = 1"));
        assert!(toml_content.contains("service_name = \"test-service\""));

        // Test TOML deserialization
        let deserialized =
            StateFile::from_toml(&toml_content).expect("Failed to deserialize from TOML");
        assert_eq!(deserialized.version, STATE_FILE_VERSION);
        assert_eq!(deserialized.state.metadata.service_name, "test-service");
    }

    #[test]
    fn test_state_file_validation_success() {
        let state = create_test_deployment_state();
        let state_file = StateFile::new(state);

        // Should validate successfully
        assert!(state_file.validate().is_ok());
    }

    #[test]
    fn test_state_metadata_validation() {
        let mut metadata = StateMetadata {
            service_name: "test-service".to_string(),
            last_deployment: Utc::now(),
            deployment_id: "dep-123".to_string(),
            provider: "aws".to_string(),
            region: "us-east-1".to_string(),
            last_config_hash: None,
            last_environment: None,
        };

        // Valid metadata should pass
        assert!(metadata.validate().is_ok());

        // Empty service name should fail
        metadata.service_name = "".to_string();
        assert!(matches!(
            metadata.validate(),
            Err(StateValidationError::InvalidServiceName(_))
        ));

        // Invalid service name characters should fail
        metadata.service_name = "test@service".to_string();
        assert!(matches!(
            metadata.validate(),
            Err(StateValidationError::InvalidServiceName(_))
        ));
    }

    #[test]
    fn test_serverless_function_validation() {
        let mut function = ServerlessFunctionState {
            name: "test-function".to_string(),
            resource_id: "arn:aws:lambda:us-east-1:123456789:function:test-function".to_string(),
            runtime: "provided.al2".to_string(),
            memory_size: 256,
            timeout: Some(30),
            environment_variables: HashMap::new(),
        };

        // Valid function should pass
        assert!(function.validate().is_ok());

        // Invalid memory size should fail
        function.memory_size = 64; // Below minimum
        assert!(matches!(
            function.validate(),
            Err(StateValidationError::InvalidMemorySize(_))
        ));

        function.memory_size = 20000; // Above maximum
        assert!(matches!(
            function.validate(),
            Err(StateValidationError::InvalidMemorySize(_))
        ));

        // Invalid timeout should fail
        function.memory_size = 256; // Reset to valid
        function.timeout = Some(0); // Invalid timeout
        assert!(matches!(
            function.validate(),
            Err(StateValidationError::InvalidTimeout(_))
        ));

        function.timeout = Some(1000); // Above maximum
        assert!(matches!(
            function.validate(),
            Err(StateValidationError::InvalidTimeout(_))
        ));
    }

    #[test]
    fn test_api_gateway_validation() {
        let mut api_gateway = ApiGatewayState {
            api_id: "abc123".to_string(),
            api_url: "https://abc123.execute-api.us-east-1.amazonaws.com".to_string(),
            stage: "prod".to_string(),
            resource_id: "arn:aws:apigateway:us-east-1::/restapis/abc123".to_string(),
        };

        // Valid API Gateway should pass
        assert!(api_gateway.validate().is_ok());

        // Invalid URL (not HTTPS) should fail
        api_gateway.api_url = "http://abc123.execute-api.us-east-1.amazonaws.com".to_string();
        assert!(matches!(
            api_gateway.validate(),
            Err(StateValidationError::InvalidApiGateway(_))
        ));

        // Empty stage should fail
        api_gateway.api_url = "https://abc123.execute-api.us-east-1.amazonaws.com".to_string(); // Reset
        api_gateway.stage = "".to_string();
        assert!(matches!(
            api_gateway.validate(),
            Err(StateValidationError::InvalidApiGateway(_))
        ));
    }

    #[test]
    fn test_message_queue_validation() {
        let mut queue = MessageQueueState {
            name: "test-queue".to_string(),
            resource_id: "https://sqs.us-east-1.amazonaws.com/123456789/test-queue".to_string(),
            topic_subscription: Some("test-topic".to_string()),
            visibility_timeout_seconds: Some(30),
            message_retention_period: Some(1209600),
            dead_letter_queue: None,
        };

        // Valid queue should pass
        assert!(queue.validate().is_ok());

        // Invalid visibility timeout should fail
        queue.visibility_timeout_seconds = Some(50000); // Above maximum
        assert!(matches!(
            queue.validate(),
            Err(StateValidationError::InvalidTimeout(_))
        ));

        // Invalid message retention period should fail
        queue.visibility_timeout_seconds = Some(30); // Reset
        queue.message_retention_period = Some(30); // Below minimum
        assert!(matches!(
            queue.validate(),
            Err(StateValidationError::InvalidTimeout(_))
        ));
    }

    #[test]
    fn test_access_role_validation() {
        let mut access_role = AccessRoleState {
            function_execution_role: "arn:aws:iam::123456789:role/test-role".to_string(),
            additional_policies: vec![],
        };

        // Valid access role should pass
        assert!(access_role.validate().is_ok());

        // Invalid ARN format should fail
        access_role.function_execution_role = "invalid-arn".to_string();
        assert!(matches!(
            access_role.validate(),
            Err(StateValidationError::InvalidRoleArn(_))
        ));

        // Empty role should fail
        access_role.function_execution_role = "".to_string();
        assert!(matches!(
            access_role.validate(),
            Err(StateValidationError::InvalidRoleArn(_))
        ));
    }

    #[test]
    fn test_state_file_migration() {
        let state = create_test_deployment_state();
        let state_file = StateFile::new(state);

        // Current version should not need migration
        let migrated = state_file.migrate().expect("Migration should succeed");
        assert_eq!(migrated.version, STATE_FILE_VERSION);

        // Test unsupported version
        let unsupported_state_file = StateFile {
            version: 999,
            state: create_test_deployment_state(),
        };

        assert!(matches!(
            unsupported_state_file.migrate(),
            Err(StateMigrationError::UnsupportedVersion(_))
        ));
    }

    #[test]
    fn test_unsupported_version_validation() {
        let state = create_test_deployment_state();
        let state_file = StateFile {
            version: 999, // Unsupported version
            state,
        };

        assert!(matches!(
            state_file.validate(),
            Err(StateValidationError::UnsupportedVersion { .. })
        ));
    }
}
