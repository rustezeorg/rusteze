use crate::cloud::*;
use crate::models::*;
use async_trait::async_trait;
use std::collections::HashMap;

/// Topic management service for automatic provisioning and configuration
pub struct TopicManager {
    messaging_client: Box<dyn MessagingClient>,
    service_name: String,
}

impl TopicManager {
    /// Create a new TopicManager
    pub fn new(messaging_client: Box<dyn MessagingClient>, service_name: String) -> Self {
        Self {
            messaging_client,
            service_name,
        }
    }

    /// Provision topics based on publisher and subscriber macros
    pub async fn provision_topics(
        &self,
        topic_plans: &[MessageTopicPlan],
        current_topics: &[MessageTopicState],
    ) -> Result<Vec<MessageTopicState>, TopicManagementError> {
        let mut provisioned_topics = Vec::new();

        for topic_plan in topic_plans {
            // Check if topic already exists
            let existing_topic = current_topics.iter().find(|t| t.name == topic_plan.name);

            let topic_state = match existing_topic {
                Some(existing) => {
                    // Topic already exists, validate configuration
                    self.validate_existing_topic(existing, topic_plan).await?;
                    existing.clone()
                }
                None => {
                    // Create new topic with dead letter queue
                    self.create_topic_with_dlq(topic_plan).await?
                }
            };

            provisioned_topics.push(topic_state);
        }

        Ok(provisioned_topics)
    }

    /// Create a new topic with dead letter queue setup
    async fn create_topic_with_dlq(
        &self,
        topic_plan: &MessageTopicPlan,
    ) -> Result<MessageTopicState, TopicManagementError> {
        println!("Creating topic: {}", topic_plan.name);

        // Create the main topic
        let mut topic_state = self
            .messaging_client
            .create_topic(topic_plan)
            .await
            .map_err(TopicManagementError::CloudError)?;

        // Create dead letter queue if specified
        if let Some(dlq_name) = &topic_plan.dead_letter_queue_name {
            println!("Creating dead letter queue: {}", dlq_name);

            let dlq_plan = MessageQueuePlan {
                name: dlq_name.clone(),
                topic_name: topic_plan.name.clone(),
                visibility_timeout_seconds: 300, // 5 minutes for DLQ
                message_retention_period: 1209600, // 14 days
                max_receive_count: 1,            // DLQ should not retry
                enable_dead_letter_queue: false, // DLQ itself doesn't need a DLQ
            };

            let dlq_state = self
                .messaging_client
                .create_queue(&dlq_plan)
                .await
                .map_err(TopicManagementError::CloudError)?;

            topic_state.dead_letter_queue_arn = Some(dlq_state.resource_id);
        }

        Ok(topic_state)
    }

    /// Validate existing topic configuration
    async fn validate_existing_topic(
        &self,
        existing_topic: &MessageTopicState,
        topic_plan: &MessageTopicPlan,
    ) -> Result<(), TopicManagementError> {
        // Validate topic name matches
        if existing_topic.name != topic_plan.name {
            return Err(TopicManagementError::ConfigurationMismatch(format!(
                "Topic name mismatch: expected {}, found {}",
                topic_plan.name, existing_topic.name
            )));
        }

        // Validate display name if specified
        if let Some(expected_display_name) = &topic_plan.display_name {
            if let Some(actual_display_name) = &existing_topic.display_name {
                if actual_display_name != expected_display_name {
                    println!(
                        "Warning: Topic display name mismatch for {}: expected {}, found {}",
                        topic_plan.name, expected_display_name, actual_display_name
                    );
                }
            }
        }

        // Validate dead letter queue configuration
        if topic_plan.dead_letter_queue_name.is_some()
            && existing_topic.dead_letter_queue_arn.is_none()
        {
            return Err(TopicManagementError::ConfigurationMismatch(format!(
                "Topic {} is missing dead letter queue configuration",
                topic_plan.name
            )));
        }

        Ok(())
    }

    /// Apply topic naming conventions
    pub fn apply_naming_conventions(&self, base_topic_name: &str) -> String {
        // Apply service prefix and sanitize name
        let sanitized_name = base_topic_name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect::<String>();

        format!("{}-{}", self.service_name, sanitized_name)
    }

    /// Get topic configuration from metadata
    pub fn extract_topic_configuration(
        &self,
        metadata: &[RouteMetadata],
    ) -> Result<Vec<TopicConfiguration>, TopicManagementError> {
        let mut topic_configs = HashMap::new();

        for route in metadata {
            match &route.handler_type {
                HandlerType::Publisher { topic } => {
                    let topic_name = self.apply_naming_conventions(topic);
                    let config = topic_configs.entry(topic_name.clone()).or_insert_with(|| {
                        TopicConfiguration {
                            name: topic_name,
                            original_name: topic.clone(),
                            has_publishers: false,
                            has_subscribers: false,
                            subscriber_queues: Vec::new(),
                        }
                    });
                    config.has_publishers = true;
                }
                HandlerType::Subscriber { topic, queue } => {
                    let topic_name = self.apply_naming_conventions(topic);
                    let queue_name = self.apply_naming_conventions(queue);
                    let config = topic_configs.entry(topic_name.clone()).or_insert_with(|| {
                        TopicConfiguration {
                            name: topic_name,
                            original_name: topic.clone(),
                            has_publishers: false,
                            has_subscribers: false,
                            subscriber_queues: Vec::new(),
                        }
                    });
                    config.has_subscribers = true;
                    config.subscriber_queues.push(queue_name);
                }
                HandlerType::HttpRoute => {}
            }
        }

        Ok(topic_configs.into_values().collect())
    }

    /// Delete unused topics
    pub async fn cleanup_unused_topics(
        &self,
        current_topics: &[MessageTopicState],
        required_topics: &[MessageTopicPlan],
    ) -> Result<Vec<String>, TopicManagementError> {
        let mut deleted_topics = Vec::new();
        let required_topic_names: std::collections::HashSet<_> =
            required_topics.iter().map(|t| &t.name).collect();

        for current_topic in current_topics {
            if !required_topic_names.contains(&current_topic.name) {
                println!("Deleting unused topic: {}", current_topic.name);

                // Delete dead letter queue first if it exists
                if let Some(dlq_arn) = &current_topic.dead_letter_queue_arn {
                    // Extract queue URL from ARN for deletion
                    // This is a simplified approach - in production you'd parse the ARN properly
                    if let Some(queue_name) = dlq_arn.split(':').last() {
                        if let Err(e) = self.messaging_client.delete_queue(queue_name).await {
                            println!(
                                "Warning: Failed to delete dead letter queue {}: {}",
                                queue_name, e
                            );
                        }
                    }
                }

                // Delete the topic
                self.messaging_client
                    .delete_topic(&current_topic.resource_id)
                    .await
                    .map_err(TopicManagementError::CloudError)?;

                deleted_topics.push(current_topic.name.clone());
            }
        }

        Ok(deleted_topics)
    }
}

/// Configuration for a topic extracted from metadata
#[derive(Debug, Clone)]
pub struct TopicConfiguration {
    pub name: String,
    pub original_name: String,
    pub has_publishers: bool,
    pub has_subscribers: bool,
    pub subscriber_queues: Vec<String>,
}

/// Queue management service for automatic provisioning and configuration
pub struct QueueManager {
    messaging_client: Box<dyn MessagingClient>,
    service_name: String,
}

impl QueueManager {
    /// Create a new QueueManager
    pub fn new(messaging_client: Box<dyn MessagingClient>, service_name: String) -> Self {
        Self {
            messaging_client,
            service_name,
        }
    }

    /// Provision queues based on subscriber macros
    pub async fn provision_queues(
        &self,
        queue_plans: &[MessageQueuePlan],
        current_queues: &[MessageQueueState],
        deployed_topics: &[MessageTopicState],
    ) -> Result<Vec<MessageQueueState>, QueueManagementError> {
        let mut provisioned_queues = Vec::new();

        for queue_plan in queue_plans {
            // Check if queue already exists
            let existing_queue = current_queues.iter().find(|q| q.name == queue_plan.name);

            let mut queue_state = match existing_queue {
                Some(existing) => {
                    // Queue already exists, validate configuration
                    self.validate_existing_queue(existing, queue_plan).await?;
                    existing.clone()
                }
                None => {
                    // Create new queue
                    self.create_queue_with_subscription(queue_plan, deployed_topics)
                        .await?
                }
            };

            // Ensure queue is subscribed to topic if not already
            if queue_state.topic_subscription.is_none() {
                queue_state = self
                    .subscribe_queue_to_topic(&queue_state, queue_plan, deployed_topics)
                    .await?;
            }

            provisioned_queues.push(queue_state);
        }

        Ok(provisioned_queues)
    }

    /// Create a new queue with topic subscription
    async fn create_queue_with_subscription(
        &self,
        queue_plan: &MessageQueuePlan,
        deployed_topics: &[MessageTopicState],
    ) -> Result<MessageQueueState, QueueManagementError> {
        println!("Creating queue: {}", queue_plan.name);

        // Create the queue
        let queue_state = self
            .messaging_client
            .create_queue(queue_plan)
            .await
            .map_err(QueueManagementError::CloudError)?;

        // Subscribe to topic
        self.subscribe_queue_to_topic(&queue_state, queue_plan, deployed_topics)
            .await
    }

    /// Subscribe queue to its associated topic
    async fn subscribe_queue_to_topic(
        &self,
        queue_state: &MessageQueueState,
        queue_plan: &MessageQueuePlan,
        deployed_topics: &[MessageTopicState],
    ) -> Result<MessageQueueState, QueueManagementError> {
        // Find the topic to subscribe to
        let topic = deployed_topics
            .iter()
            .find(|t| t.name == queue_plan.topic_name)
            .ok_or_else(|| QueueManagementError::TopicNotFound(queue_plan.topic_name.clone()))?;

        println!(
            "Subscribing queue {} to topic {}",
            queue_state.name, topic.name
        );

        // Create subscription
        let subscription_arn = self
            .messaging_client
            .subscribe_queue_to_topic(&queue_state.resource_id, &topic.resource_id)
            .await
            .map_err(QueueManagementError::CloudError)?;

        // Return updated queue state with subscription
        let mut updated_queue = queue_state.clone();
        updated_queue.topic_subscription = Some(subscription_arn);

        Ok(updated_queue)
    }

    /// Validate existing queue configuration
    async fn validate_existing_queue(
        &self,
        existing_queue: &MessageQueueState,
        queue_plan: &MessageQueuePlan,
    ) -> Result<(), QueueManagementError> {
        // Validate queue name matches
        if existing_queue.name != queue_plan.name {
            return Err(QueueManagementError::ConfigurationMismatch(format!(
                "Queue name mismatch: expected {}, found {}",
                queue_plan.name, existing_queue.name
            )));
        }

        // Validate visibility timeout
        if let Some(existing_timeout) = existing_queue.visibility_timeout_seconds {
            if existing_timeout != queue_plan.visibility_timeout_seconds {
                println!(
                    "Warning: Queue visibility timeout mismatch for {}: expected {}, found {}",
                    queue_plan.name, queue_plan.visibility_timeout_seconds, existing_timeout
                );
            }
        }

        // Validate message retention period
        if let Some(existing_retention) = existing_queue.message_retention_period {
            if existing_retention != queue_plan.message_retention_period {
                println!(
                    "Warning: Queue message retention mismatch for {}: expected {}, found {}",
                    queue_plan.name, queue_plan.message_retention_period, existing_retention
                );
            }
        }

        // Validate dead letter queue configuration
        if queue_plan.enable_dead_letter_queue && existing_queue.dead_letter_queue.is_none() {
            return Err(QueueManagementError::ConfigurationMismatch(format!(
                "Queue {} is missing dead letter queue configuration",
                queue_plan.name
            )));
        }

        Ok(())
    }

    /// Apply queue naming conventions
    pub fn apply_naming_conventions(&self, base_queue_name: &str) -> String {
        // Apply service prefix and sanitize name
        let sanitized_name = base_queue_name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect::<String>();

        format!("{}-{}", self.service_name, sanitized_name)
    }

    /// Configure queue retry policies
    pub fn configure_retry_policies(
        &self,
        queue_plan: &MessageQueuePlan,
    ) -> QueueRetryConfiguration {
        QueueRetryConfiguration {
            max_receive_count: queue_plan.max_receive_count,
            visibility_timeout_seconds: queue_plan.visibility_timeout_seconds,
            message_retention_period: queue_plan.message_retention_period,
            enable_dead_letter_queue: queue_plan.enable_dead_letter_queue,
            retry_delay_seconds: 30, // Default retry delay
            max_retry_attempts: 3,   // Default max retries
        }
    }

    /// Setup event source mapping for serverless functions
    pub async fn setup_event_source_mapping(
        &self,
        queue_state: &MessageQueueState,
        function_arn: &str,
    ) -> Result<String, QueueManagementError> {
        // In a real implementation, this would create an event source mapping
        // between the SQS queue and the Lambda function
        println!(
            "Setting up event source mapping: queue {} -> function {}",
            queue_state.name, function_arn
        );

        // Return a mock event source mapping ARN
        Ok(format!(
            "arn:aws:lambda:us-east-1:123456789:event-source-mapping:{}",
            uuid::Uuid::new_v4().to_string()[..8].to_string()
        ))
    }

    /// Delete unused queues
    pub async fn cleanup_unused_queues(
        &self,
        current_queues: &[MessageQueueState],
        required_queues: &[MessageQueuePlan],
    ) -> Result<Vec<String>, QueueManagementError> {
        let mut deleted_queues = Vec::new();
        let required_queue_names: std::collections::HashSet<_> =
            required_queues.iter().map(|q| &q.name).collect();

        for current_queue in current_queues {
            if !required_queue_names.contains(&current_queue.name) {
                println!("Deleting unused queue: {}", current_queue.name);

                // Unsubscribe from topic first if subscribed
                if let Some(subscription_arn) = &current_queue.topic_subscription {
                    if let Err(e) = self
                        .messaging_client
                        .unsubscribe_queue_from_topic(subscription_arn)
                        .await
                    {
                        println!(
                            "Warning: Failed to unsubscribe queue {}: {}",
                            current_queue.name, e
                        );
                    }
                }

                // Delete the queue
                self.messaging_client
                    .delete_queue(&current_queue.resource_id)
                    .await
                    .map_err(QueueManagementError::CloudError)?;

                deleted_queues.push(current_queue.name.clone());
            }
        }

        Ok(deleted_queues)
    }
}

/// Configuration for queue retry policies
#[derive(Debug, Clone)]
pub struct QueueRetryConfiguration {
    pub max_receive_count: u32,
    pub visibility_timeout_seconds: u32,
    pub message_retention_period: u32,
    pub enable_dead_letter_queue: bool,
    pub retry_delay_seconds: u32,
    pub max_retry_attempts: u32,
}

/// Errors that can occur during queue management
#[derive(Debug, thiserror::Error)]
pub enum QueueManagementError {
    #[error("Cloud operation failed: {0}")]
    CloudError(#[from] CloudError),

    #[error("Configuration mismatch: {0}")]
    ConfigurationMismatch(String),

    #[error("Invalid queue name: {0}")]
    InvalidQueueName(String),

    #[error("Queue not found: {0}")]
    QueueNotFound(String),

    #[error("Topic not found: {0}")]
    TopicNotFound(String),

    #[error("Subscription failed: {0}")]
    SubscriptionFailed(String),

    #[error("Event source mapping failed: {0}")]
    EventSourceMappingFailed(String),

    #[error("Queue validation failed: {0}")]
    ValidationFailed(String),
}

impl QueueManagementError {
    /// Check if the error is retryable
    pub fn is_retryable(&self) -> bool {
        match self {
            QueueManagementError::CloudError(cloud_error) => cloud_error.is_retryable(),
            _ => false,
        }
    }

    /// Get suggested remediation steps for the error
    pub fn remediation_steps(&self) -> Vec<String> {
        match self {
            QueueManagementError::CloudError(cloud_error) => cloud_error.remediation_steps(),
            QueueManagementError::ConfigurationMismatch(_) => vec![
                "Review your queue configuration in rusteze.toml".to_string(),
                "Ensure queue names are consistent with subscriber macros".to_string(),
                "Check for naming conflicts with existing resources".to_string(),
            ],
            QueueManagementError::InvalidQueueName(_) => vec![
                "Use only alphanumeric characters and hyphens in queue names".to_string(),
                "Ensure queue names are not empty".to_string(),
                "Follow cloud provider naming conventions".to_string(),
            ],
            QueueManagementError::QueueNotFound(_) => vec![
                "Verify the queue name is correct".to_string(),
                "Check if the queue exists in the correct region".to_string(),
                "Ensure you have permission to access the queue".to_string(),
            ],
            QueueManagementError::TopicNotFound(_) => vec![
                "Ensure the topic is created before creating queues".to_string(),
                "Verify the topic name matches the queue configuration".to_string(),
                "Check that topics are deployed in the correct order".to_string(),
            ],
            QueueManagementError::SubscriptionFailed(_) => vec![
                "Check your permissions for SNS and SQS operations".to_string(),
                "Verify the topic and queue exist in the same region".to_string(),
                "Ensure the subscription policy allows the connection".to_string(),
            ],
            QueueManagementError::EventSourceMappingFailed(_) => vec![
                "Check your Lambda function permissions".to_string(),
                "Verify the queue exists and is accessible".to_string(),
                "Ensure the function execution role has SQS permissions".to_string(),
            ],
            QueueManagementError::ValidationFailed(_) => vec![
                "Review the queue configuration for consistency".to_string(),
                "Check that all required fields are provided".to_string(),
                "Validate against cloud provider requirements".to_string(),
            ],
        }
    }
}

/// Errors that can occur during topic management
#[derive(Debug, thiserror::Error)]
pub enum TopicManagementError {
    #[error("Cloud operation failed: {0}")]
    CloudError(#[from] CloudError),

    #[error("Configuration mismatch: {0}")]
    ConfigurationMismatch(String),

    #[error("Invalid topic name: {0}")]
    InvalidTopicName(String),

    #[error("Topic not found: {0}")]
    TopicNotFound(String),

    #[error("Dead letter queue setup failed: {0}")]
    DeadLetterQueueFailed(String),

    #[error("Topic validation failed: {0}")]
    ValidationFailed(String),
}

impl TopicManagementError {
    /// Check if the error is retryable
    pub fn is_retryable(&self) -> bool {
        match self {
            TopicManagementError::CloudError(cloud_error) => cloud_error.is_retryable(),
            _ => false,
        }
    }

    /// Get suggested remediation steps for the error
    pub fn remediation_steps(&self) -> Vec<String> {
        match self {
            TopicManagementError::CloudError(cloud_error) => cloud_error.remediation_steps(),
            TopicManagementError::ConfigurationMismatch(_) => vec![
                "Review your topic configuration in rusteze.toml".to_string(),
                "Ensure topic names are consistent across publishers and subscribers".to_string(),
                "Check for naming conflicts with existing resources".to_string(),
            ],
            TopicManagementError::InvalidTopicName(_) => vec![
                "Use only alphanumeric characters and hyphens in topic names".to_string(),
                "Ensure topic names are not empty".to_string(),
                "Follow cloud provider naming conventions".to_string(),
            ],
            TopicManagementError::TopicNotFound(_) => vec![
                "Verify the topic name is correct".to_string(),
                "Check if the topic exists in the correct region".to_string(),
                "Ensure you have permission to access the topic".to_string(),
            ],
            TopicManagementError::DeadLetterQueueFailed(_) => vec![
                "Check your permissions for creating SQS queues".to_string(),
                "Verify the dead letter queue configuration".to_string(),
                "Ensure the queue name doesn't conflict with existing resources".to_string(),
            ],
            TopicManagementError::ValidationFailed(_) => vec![
                "Review the topic configuration for consistency".to_string(),
                "Check that all required fields are provided".to_string(),
                "Validate against cloud provider requirements".to_string(),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MockMessagingClient;

    #[tokio::test]
    async fn test_topic_naming_conventions() {
        let mock_client = Box::new(MockMessagingClient::new());
        let topic_manager = TopicManager::new(mock_client, "test-service".to_string());

        let result = topic_manager.apply_naming_conventions("user-events");
        assert_eq!(result, "test-service-user-events");

        let result = topic_manager.apply_naming_conventions("user_events");
        assert_eq!(result, "test-service-user-events");

        let result = topic_manager.apply_naming_conventions("user@events");
        assert_eq!(result, "test-service-user-events");
    }

    #[tokio::test]
    async fn test_extract_topic_configuration() {
        let mock_client = Box::new(MockMessagingClient::new());
        let topic_manager = TopicManager::new(mock_client, "test-service".to_string());

        let metadata = vec![
            RouteMetadata {
                function_name: "publish_user_event".to_string(),
                method: "POST".to_string(),
                path: "/publish".to_string(),
                handler_type: HandlerType::Publisher {
                    topic: "user-events".to_string(),
                },
            },
            RouteMetadata {
                function_name: "handle_user_event".to_string(),
                method: "POST".to_string(),
                path: "/subscribe".to_string(),
                handler_type: HandlerType::Subscriber {
                    topic: "user-events".to_string(),
                    queue: "user-processor".to_string(),
                },
            },
        ];

        let configs = topic_manager
            .extract_topic_configuration(&metadata)
            .unwrap();
        assert_eq!(configs.len(), 1);

        let config = &configs[0];
        assert_eq!(config.name, "test-service-user-events");
        assert_eq!(config.original_name, "user-events");
        assert!(config.has_publishers);
        assert!(config.has_subscribers);
        assert_eq!(config.subscriber_queues.len(), 1);
        assert_eq!(config.subscriber_queues[0], "test-service-user-processor");
    }

    #[tokio::test]
    async fn test_queue_naming_conventions() {
        let mock_client = Box::new(MockMessagingClient::new());
        let queue_manager = QueueManager::new(mock_client, "test-service".to_string());

        let result = queue_manager.apply_naming_conventions("user-processor");
        assert_eq!(result, "test-service-user-processor");

        let result = queue_manager.apply_naming_conventions("user_processor");
        assert_eq!(result, "test-service-user-processor");

        let result = queue_manager.apply_naming_conventions("user@processor");
        assert_eq!(result, "test-service-user-processor");
    }

    #[tokio::test]
    async fn test_queue_retry_configuration() {
        let mock_client = Box::new(MockMessagingClient::new());
        let queue_manager = QueueManager::new(mock_client, "test-service".to_string());

        let queue_plan = MessageQueuePlan {
            name: "test-queue".to_string(),
            topic_name: "test-topic".to_string(),
            visibility_timeout_seconds: 60,
            message_retention_period: 86400,
            max_receive_count: 5,
            enable_dead_letter_queue: true,
        };

        let retry_config = queue_manager.configure_retry_policies(&queue_plan);
        assert_eq!(retry_config.max_receive_count, 5);
        assert_eq!(retry_config.visibility_timeout_seconds, 60);
        assert_eq!(retry_config.message_retention_period, 86400);
        assert!(retry_config.enable_dead_letter_queue);
        assert_eq!(retry_config.retry_delay_seconds, 30);
        assert_eq!(retry_config.max_retry_attempts, 3);
    }

    #[tokio::test]
    async fn test_provision_queues() {
        let mock_client = Box::new(MockMessagingClient::new());
        let queue_manager = QueueManager::new(mock_client, "test-service".to_string());

        let queue_plans = vec![MessageQueuePlan {
            name: "test-queue".to_string(),
            topic_name: "test-topic".to_string(),
            visibility_timeout_seconds: 30,
            message_retention_period: 1209600,
            max_receive_count: 3,
            enable_dead_letter_queue: true,
        }];

        let current_queues = vec![];

        let deployed_topics = vec![MessageTopicState {
            name: "test-topic".to_string(),
            resource_id: "arn:aws:sns:us-east-1:123456789:test-topic".to_string(),
            display_name: Some("Test Topic".to_string()),
            dead_letter_queue_arn: None,
        }];

        let result = queue_manager
            .provision_queues(&queue_plans, &current_queues, &deployed_topics)
            .await;

        assert!(result.is_ok());
        let provisioned_queues = result.unwrap();
        assert_eq!(provisioned_queues.len(), 1);

        let queue = &provisioned_queues[0];
        assert_eq!(queue.name, "test-queue");
        assert!(queue.topic_subscription.is_some());
    }
}
