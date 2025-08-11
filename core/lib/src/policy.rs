use crate::cloud::*;
use crate::models::*;
use std::collections::HashMap;

/// Policy management service for automatic IAM/access policy creation
pub struct PolicyManager {
    access_control_client: Box<dyn AccessControlClient>,
    service_name: String,
}

impl PolicyManager {
    /// Create a new PolicyManager
    pub fn new(access_control_client: Box<dyn AccessControlClient>, service_name: String) -> Self {
        Self {
            access_control_client,
            service_name,
        }
    }

    /// Generate access policies for pub/sub operations
    pub fn generate_pubsub_policies(
        &self,
        topics: &[MessageTopicState],
        queues: &[MessageQueueState],
        functions: &[ServerlessFunctionState],
    ) -> Result<Vec<AccessPolicyPlan>, PolicyManagementError> {
        let mut policies = Vec::new();

        // Generate SNS publish policy if there are topics
        if !topics.is_empty() {
            let sns_policy = self.create_sns_publish_policy(topics)?;
            policies.push(sns_policy);
        }

        // Generate SQS consume policy if there are queues
        if !queues.is_empty() {
            let sqs_policy = self.create_sqs_consume_policy(queues)?;
            policies.push(sqs_policy);
        }

        // Generate Lambda execution policy
        if !functions.is_empty() {
            let lambda_policy = self.create_lambda_execution_policy()?;
            policies.push(lambda_policy);
        }

        // Generate CloudWatch logs policy for all functions
        if !functions.is_empty() {
            let logs_policy = self.create_cloudwatch_logs_policy()?;
            policies.push(logs_policy);
        }

        Ok(policies)
    }

    /// Create SNS publish policy template
    fn create_sns_publish_policy(
        &self,
        topics: &[MessageTopicState],
    ) -> Result<AccessPolicyPlan, PolicyManagementError> {
        let topic_arns: Vec<String> = topics.iter().map(|t| t.resource_id.clone()).collect();

        if topic_arns.is_empty() {
            return Err(PolicyManagementError::InvalidConfiguration(
                "No topics provided for SNS publish policy".to_string(),
            ));
        }

        let policy_document = self.create_sns_publish_policy_document(&topic_arns)?;

        Ok(AccessPolicyPlan {
            name: format!("{}-sns-publish-policy", self.service_name),
            policy_document,
            attached_to: vec![format!("{}-execution-role", self.service_name)],
        })
    }

    /// Create SQS consume policy template
    fn create_sqs_consume_policy(
        &self,
        queues: &[MessageQueueState],
    ) -> Result<AccessPolicyPlan, PolicyManagementError> {
        let queue_arns: Vec<String> = queues
            .iter()
            .map(|q| {
                // Convert SQS URL to ARN format
                self.convert_sqs_url_to_arn(&q.resource_id)
            })
            .collect();

        if queue_arns.is_empty() {
            return Err(PolicyManagementError::InvalidConfiguration(
                "No queues provided for SQS consume policy".to_string(),
            ));
        }

        let policy_document = self.create_sqs_consume_policy_document(&queue_arns)?;

        Ok(AccessPolicyPlan {
            name: format!("{}-sqs-consume-policy", self.service_name),
            policy_document,
            attached_to: vec![format!("{}-execution-role", self.service_name)],
        })
    }

    /// Create Lambda execution policy
    fn create_lambda_execution_policy(&self) -> Result<AccessPolicyPlan, PolicyManagementError> {
        let policy_document = r#"{
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Effect": "Allow",
                    "Action": [
                        "lambda:InvokeFunction"
                    ],
                    "Resource": "*"
                }
            ]
        }"#
        .to_string();

        Ok(AccessPolicyPlan {
            name: format!("{}-lambda-execution-policy", self.service_name),
            policy_document,
            attached_to: vec![format!("{}-execution-role", self.service_name)],
        })
    }

    /// Create CloudWatch logs policy
    fn create_cloudwatch_logs_policy(&self) -> Result<AccessPolicyPlan, PolicyManagementError> {
        let policy_document = format!(
            r#"{{
                "Version": "2012-10-17",
                "Statement": [
                    {{
                        "Effect": "Allow",
                        "Action": [
                            "logs:CreateLogGroup",
                            "logs:CreateLogStream",
                            "logs:PutLogEvents"
                        ],
                        "Resource": "arn:aws:logs:*:*:log-group:/aws/lambda/{}*"
                    }}
                ]
            }}"#,
            self.service_name
        );

        Ok(AccessPolicyPlan {
            name: format!("{}-cloudwatch-logs-policy", self.service_name),
            policy_document,
            attached_to: vec![format!("{}-execution-role", self.service_name)],
        })
    }

    /// Create SNS publish policy document
    fn create_sns_publish_policy_document(
        &self,
        topic_arns: &[String],
    ) -> Result<String, PolicyManagementError> {
        let topic_resources = topic_arns
            .iter()
            .map(|arn| format!("\"{}\"", arn))
            .collect::<Vec<_>>()
            .join(",");

        let policy_document = format!(
            r#"{{
                "Version": "2012-10-17",
                "Statement": [
                    {{
                        "Effect": "Allow",
                        "Action": [
                            "sns:Publish",
                            "sns:GetTopicAttributes"
                        ],
                        "Resource": [
                            {}
                        ]
                    }}
                ]
            }}"#,
            topic_resources
        );

        Ok(policy_document)
    }

    /// Create SQS consume policy document
    fn create_sqs_consume_policy_document(
        &self,
        queue_arns: &[String],
    ) -> Result<String, PolicyManagementError> {
        let queue_resources = queue_arns
            .iter()
            .map(|arn| format!("\"{}\"", arn))
            .collect::<Vec<_>>()
            .join(",");

        let policy_document = format!(
            r#"{{
                "Version": "2012-10-17",
                "Statement": [
                    {{
                        "Effect": "Allow",
                        "Action": [
                            "sqs:ReceiveMessage",
                            "sqs:DeleteMessage",
                            "sqs:GetQueueAttributes",
                            "sqs:ChangeMessageVisibility"
                        ],
                        "Resource": [
                            {}
                        ]
                    }}
                ]
            }}"#,
            queue_resources
        );

        Ok(policy_document)
    }

    /// Convert SQS URL to ARN format
    fn convert_sqs_url_to_arn(&self, sqs_url: &str) -> String {
        // Extract queue name from URL: https://sqs.region.amazonaws.com/account/queue-name
        if let Some(queue_name) = sqs_url.split('/').last() {
            // Extract region and account from URL
            let parts: Vec<&str> = sqs_url.split('/').collect();
            if parts.len() >= 4 {
                let domain_parts: Vec<&str> = parts[2].split('.').collect();
                if domain_parts.len() >= 3 {
                    let region = domain_parts[1];
                    let account = parts[3];
                    return format!("arn:aws:sqs:{}:{}:{}", region, account, queue_name);
                }
            }
        }

        // Fallback: assume us-east-1 and generic account
        format!(
            "arn:aws:sqs:us-east-1:123456789:{}",
            sqs_url.split('/').last().unwrap_or("unknown-queue")
        )
    }

    /// Attach policies to serverless function execution roles
    pub async fn attach_policies_to_functions(
        &self,
        policies: &[AccessPolicyPlan],
        functions: &[ServerlessFunctionState],
        execution_role_arn: &str,
    ) -> Result<Vec<String>, PolicyManagementError> {
        let mut attached_policies = Vec::new();

        for policy in policies {
            // Create the policy
            let policy_arn = self
                .access_control_client
                .create_policy(policy)
                .await
                .map_err(PolicyManagementError::CloudError)?;

            // Attach policy to execution role
            self.access_control_client
                .attach_policy_to_role(execution_role_arn, &policy_arn)
                .await
                .map_err(PolicyManagementError::CloudError)?;

            attached_policies.push(policy_arn);

            println!(
                "Attached policy {} to role {}",
                policy.name, execution_role_arn
            );
        }

        Ok(attached_policies)
    }

    /// Create comprehensive execution role with all required policies
    pub async fn create_execution_role_with_policies(
        &self,
        topics: &[MessageTopicState],
        queues: &[MessageQueueState],
        functions: &[ServerlessFunctionState],
    ) -> Result<AccessRoleState, PolicyManagementError> {
        // Generate all required policies
        let policies = self.generate_pubsub_policies(topics, queues, functions)?;

        // Create execution role with policies
        let role_arn = self
            .access_control_client
            .create_execution_role(&self.service_name, &policies)
            .await
            .map_err(PolicyManagementError::CloudError)?;

        // Attach additional policies to the role
        let attached_policies = self
            .attach_policies_to_functions(&policies, functions, &role_arn)
            .await?;

        Ok(AccessRoleState {
            function_execution_role: role_arn,
            additional_policies: attached_policies,
        })
    }

    /// Update existing execution role with new policies
    pub async fn update_execution_role_policies(
        &self,
        role_arn: &str,
        topics: &[MessageTopicState],
        queues: &[MessageQueueState],
        functions: &[ServerlessFunctionState],
    ) -> Result<Vec<String>, PolicyManagementError> {
        // Generate updated policies
        let policies = self.generate_pubsub_policies(topics, queues, functions)?;

        // Update execution role with new policies
        self.access_control_client
            .update_execution_role(role_arn, &policies)
            .await
            .map_err(PolicyManagementError::CloudError)?;

        // Attach updated policies
        let attached_policies = self
            .attach_policies_to_functions(&policies, functions, role_arn)
            .await?;

        Ok(attached_policies)
    }

    /// Generate policy templates for different resource types
    pub fn generate_policy_templates(&self) -> HashMap<String, PolicyTemplate> {
        let mut templates = HashMap::new();

        // SNS Publish Template
        templates.insert(
            "sns-publish".to_string(),
            PolicyTemplate {
                name: "SNS Publish Policy".to_string(),
                description: "Allows publishing messages to SNS topics".to_string(),
                actions: vec![
                    "sns:Publish".to_string(),
                    "sns:GetTopicAttributes".to_string(),
                ],
                resource_type: "sns:topic".to_string(),
                conditions: HashMap::new(),
            },
        );

        // SQS Consume Template
        templates.insert(
            "sqs-consume".to_string(),
            PolicyTemplate {
                name: "SQS Consume Policy".to_string(),
                description: "Allows consuming messages from SQS queues".to_string(),
                actions: vec![
                    "sqs:ReceiveMessage".to_string(),
                    "sqs:DeleteMessage".to_string(),
                    "sqs:GetQueueAttributes".to_string(),
                    "sqs:ChangeMessageVisibility".to_string(),
                ],
                resource_type: "sqs:queue".to_string(),
                conditions: HashMap::new(),
            },
        );

        // Lambda Execution Template
        templates.insert(
            "lambda-execution".to_string(),
            PolicyTemplate {
                name: "Lambda Execution Policy".to_string(),
                description: "Basic Lambda execution permissions".to_string(),
                actions: vec!["lambda:InvokeFunction".to_string()],
                resource_type: "lambda:function".to_string(),
                conditions: HashMap::new(),
            },
        );

        // CloudWatch Logs Template
        templates.insert(
            "cloudwatch-logs".to_string(),
            PolicyTemplate {
                name: "CloudWatch Logs Policy".to_string(),
                description: "Allows writing logs to CloudWatch".to_string(),
                actions: vec![
                    "logs:CreateLogGroup".to_string(),
                    "logs:CreateLogStream".to_string(),
                    "logs:PutLogEvents".to_string(),
                ],
                resource_type: "logs:log-group".to_string(),
                conditions: HashMap::new(),
            },
        );

        templates
    }

    /// Validate policy configuration
    pub fn validate_policy_configuration(
        &self,
        policy: &AccessPolicyPlan,
    ) -> Result<(), PolicyManagementError> {
        // Validate policy name
        if policy.name.is_empty() {
            return Err(PolicyManagementError::InvalidConfiguration(
                "Policy name cannot be empty".to_string(),
            ));
        }

        // Validate policy document is valid JSON
        if let Err(e) = serde_json::from_str::<serde_json::Value>(&policy.policy_document) {
            return Err(PolicyManagementError::InvalidConfiguration(format!(
                "Invalid policy document JSON: {}",
                e
            )));
        }

        // Validate attached_to is not empty
        if policy.attached_to.is_empty() {
            return Err(PolicyManagementError::InvalidConfiguration(
                "Policy must be attached to at least one role".to_string(),
            ));
        }

        Ok(())
    }
}

/// Policy template for generating standardized policies
#[derive(Debug, Clone)]
pub struct PolicyTemplate {
    pub name: String,
    pub description: String,
    pub actions: Vec<String>,
    pub resource_type: String,
    pub conditions: HashMap<String, String>,
}

/// Errors that can occur during policy management
#[derive(Debug, thiserror::Error)]
pub enum PolicyManagementError {
    #[error("Cloud operation failed: {0}")]
    CloudError(#[from] CloudError),

    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    #[error("Policy validation failed: {0}")]
    ValidationFailed(String),

    #[error("Policy creation failed: {0}")]
    PolicyCreationFailed(String),

    #[error("Role attachment failed: {0}")]
    RoleAttachmentFailed(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),
}

impl PolicyManagementError {
    /// Check if the error is retryable
    pub fn is_retryable(&self) -> bool {
        match self {
            PolicyManagementError::CloudError(cloud_error) => cloud_error.is_retryable(),
            _ => false,
        }
    }

    /// Get suggested remediation steps for the error
    pub fn remediation_steps(&self) -> Vec<String> {
        match self {
            PolicyManagementError::CloudError(cloud_error) => cloud_error.remediation_steps(),
            PolicyManagementError::InvalidConfiguration(_) => vec![
                "Review your policy configuration".to_string(),
                "Ensure all required fields are provided".to_string(),
                "Validate JSON syntax in policy documents".to_string(),
            ],
            PolicyManagementError::ValidationFailed(_) => vec![
                "Check policy document syntax".to_string(),
                "Ensure policy actions are valid".to_string(),
                "Verify resource ARNs are correctly formatted".to_string(),
            ],
            PolicyManagementError::PolicyCreationFailed(_) => vec![
                "Check your IAM permissions".to_string(),
                "Ensure policy names are unique".to_string(),
                "Verify policy document is valid".to_string(),
            ],
            PolicyManagementError::RoleAttachmentFailed(_) => vec![
                "Check your IAM permissions for role management".to_string(),
                "Ensure the role exists".to_string(),
                "Verify the policy exists and is valid".to_string(),
            ],
            PolicyManagementError::PermissionDenied(_) => vec![
                "Check your IAM permissions".to_string(),
                "Ensure you have the required access to create policies and roles".to_string(),
                "Contact your administrator for additional permissions".to_string(),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aws_mock::MockAwsAccessControlClient;

    #[tokio::test]
    async fn test_generate_sns_publish_policy() {
        let mock_client = Box::new(MockAwsAccessControlClient::new());
        let policy_manager = PolicyManager::new(mock_client, "test-service".to_string());

        let topics = vec![MessageTopicState {
            name: "test-topic".to_string(),
            resource_id: "arn:aws:sns:us-east-1:123456789:test-topic".to_string(),
            display_name: Some("Test Topic".to_string()),
            dead_letter_queue_arn: None,
        }];

        let result = policy_manager.create_sns_publish_policy(&topics);
        assert!(result.is_ok());

        let policy = result.unwrap();
        assert_eq!(policy.name, "test-service-sns-publish-policy");
        assert!(policy.policy_document.contains("sns:Publish"));
        assert!(
            policy
                .policy_document
                .contains("arn:aws:sns:us-east-1:123456789:test-topic")
        );
    }

    #[tokio::test]
    async fn test_generate_sqs_consume_policy() {
        let mock_client = Box::new(MockAwsAccessControlClient::new());
        let policy_manager = PolicyManager::new(mock_client, "test-service".to_string());

        let queues = vec![MessageQueueState {
            name: "test-queue".to_string(),
            resource_id: "https://sqs.us-east-1.amazonaws.com/123456789/test-queue".to_string(),
            topic_subscription: None,
            visibility_timeout_seconds: Some(30),
            message_retention_period: Some(1209600),
            dead_letter_queue: None,
        }];

        let result = policy_manager.create_sqs_consume_policy(&queues);
        assert!(result.is_ok());

        let policy = result.unwrap();
        assert_eq!(policy.name, "test-service-sqs-consume-policy");
        assert!(policy.policy_document.contains("sqs:ReceiveMessage"));
        assert!(
            policy
                .policy_document
                .contains("arn:aws:sqs:us-east-1:123456789:test-queue")
        );
    }

    #[tokio::test]
    async fn test_convert_sqs_url_to_arn() {
        let mock_client = Box::new(MockAwsAccessControlClient::new());
        let policy_manager = PolicyManager::new(mock_client, "test-service".to_string());

        let sqs_url = "https://sqs.us-east-1.amazonaws.com/123456789/test-queue";
        let arn = policy_manager.convert_sqs_url_to_arn(sqs_url);
        assert_eq!(arn, "arn:aws:sqs:us-east-1:123456789:test-queue");
    }

    #[tokio::test]
    async fn test_policy_validation() {
        let mock_client = Box::new(MockAwsAccessControlClient::new());
        let policy_manager = PolicyManager::new(mock_client, "test-service".to_string());

        // Valid policy
        let valid_policy = AccessPolicyPlan {
            name: "test-policy".to_string(),
            policy_document: r#"{"Version": "2012-10-17", "Statement": []}"#.to_string(),
            attached_to: vec!["test-role".to_string()],
        };

        assert!(
            policy_manager
                .validate_policy_configuration(&valid_policy)
                .is_ok()
        );

        // Invalid policy - empty name
        let invalid_policy = AccessPolicyPlan {
            name: "".to_string(),
            policy_document: r#"{"Version": "2012-10-17", "Statement": []}"#.to_string(),
            attached_to: vec!["test-role".to_string()],
        };

        assert!(
            policy_manager
                .validate_policy_configuration(&invalid_policy)
                .is_err()
        );

        // Invalid policy - malformed JSON
        let invalid_json_policy = AccessPolicyPlan {
            name: "test-policy".to_string(),
            policy_document: "invalid json".to_string(),
            attached_to: vec!["test-role".to_string()],
        };

        assert!(
            policy_manager
                .validate_policy_configuration(&invalid_json_policy)
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_generate_policy_templates() {
        let mock_client = Box::new(MockAwsAccessControlClient::new());
        let policy_manager = PolicyManager::new(mock_client, "test-service".to_string());

        let templates = policy_manager.generate_policy_templates();

        assert!(templates.contains_key("sns-publish"));
        assert!(templates.contains_key("sqs-consume"));
        assert!(templates.contains_key("lambda-execution"));
        assert!(templates.contains_key("cloudwatch-logs"));

        let sns_template = &templates["sns-publish"];
        assert_eq!(sns_template.name, "SNS Publish Policy");
        assert!(sns_template.actions.contains(&"sns:Publish".to_string()));
    }
}
