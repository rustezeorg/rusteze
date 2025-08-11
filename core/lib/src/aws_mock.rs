use crate::cloud::*;
use crate::models::*;
use async_trait::async_trait;
use std::collections::HashMap;

/// Mock AWS implementation for testing and development
pub struct MockAwsCloudClients {
    serverless: MockAwsServerlessClient,
    api_gateway: MockAwsApiGatewayClient,
    messaging: MockAwsMessagingClient,
    access_control: MockAwsAccessControlClient,
}

impl MockAwsCloudClients {
    pub fn new() -> Self {
        Self {
            serverless: MockAwsServerlessClient::new(),
            api_gateway: MockAwsApiGatewayClient::new(),
            messaging: MockAwsMessagingClient::new(),
            access_control: MockAwsAccessControlClient::new(),
        }
    }
}

#[async_trait]
impl CloudClients for MockAwsCloudClients {
    fn serverless(&self) -> &dyn ServerlessClient {
        &self.serverless
    }

    fn api_gateway(&self) -> &dyn ApiGatewayClient {
        &self.api_gateway
    }

    fn messaging(&self) -> &dyn MessagingClient {
        &self.messaging
    }

    fn access_control(&self) -> &dyn AccessControlClient {
        &self.access_control
    }
}

/// Mock AWS Lambda client
pub struct MockAwsServerlessClient {
    functions: std::sync::Mutex<HashMap<String, ServerlessFunctionState>>,
}

impl MockAwsServerlessClient {
    pub fn new() -> Self {
        Self {
            functions: std::sync::Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl ServerlessClient for MockAwsServerlessClient {
    async fn create_function(
        &self,
        plan: &ServerlessFunctionPlan,
    ) -> Result<ServerlessFunctionState, CloudError> {
        let function_state = ServerlessFunctionState {
            name: plan.name.clone(),
            resource_id: format!("arn:aws:lambda:us-east-1:123456789:function:{}", plan.name),
            runtime: plan.runtime.clone(),
            memory_size: plan.memory_size,
            timeout: Some(plan.timeout),
            environment_variables: plan.environment_variables.clone(),
        };

        let mut functions = self.functions.lock().unwrap();
        functions.insert(plan.name.clone(), function_state.clone());

        Ok(function_state)
    }

    async fn update_function(
        &self,
        name: &str,
        plan: &ServerlessFunctionPlan,
    ) -> Result<ServerlessFunctionState, CloudError> {
        let function_state = ServerlessFunctionState {
            name: name.to_string(),
            resource_id: format!("arn:aws:lambda:us-east-1:123456789:function:{}", name),
            runtime: plan.runtime.clone(),
            memory_size: plan.memory_size,
            timeout: Some(plan.timeout),
            environment_variables: plan.environment_variables.clone(),
        };

        let mut functions = self.functions.lock().unwrap();
        functions.insert(name.to_string(), function_state.clone());

        Ok(function_state)
    }

    async fn delete_function(&self, name: &str) -> Result<(), CloudError> {
        let mut functions = self.functions.lock().unwrap();
        functions.remove(name);
        Ok(())
    }

    async fn get_function(
        &self,
        name: &str,
    ) -> Result<Option<ServerlessFunctionState>, CloudError> {
        let functions = self.functions.lock().unwrap();
        Ok(functions.get(name).cloned())
    }

    async fn list_functions(&self) -> Result<Vec<ServerlessFunctionState>, CloudError> {
        let functions = self.functions.lock().unwrap();
        Ok(functions.values().cloned().collect())
    }

    async fn update_function_configuration(
        &self,
        name: &str,
        config: &FunctionConfiguration,
    ) -> Result<(), CloudError> {
        let mut functions = self.functions.lock().unwrap();
        if let Some(function) = functions.get_mut(name) {
            if let Some(memory_size) = config.memory_size {
                function.memory_size = memory_size;
            }
            if let Some(timeout) = config.timeout {
                function.timeout = Some(timeout);
            }
            if let Some(ref env_vars) = config.environment_variables {
                function.environment_variables = env_vars.clone();
            }
        }
        Ok(())
    }
}

/// Mock AWS API Gateway client
pub struct MockAwsApiGatewayClient {
    apis: std::sync::Mutex<HashMap<String, ApiGatewayState>>,
}

impl MockAwsApiGatewayClient {
    pub fn new() -> Self {
        Self {
            apis: std::sync::Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl ApiGatewayClient for MockAwsApiGatewayClient {
    async fn create_api(&self, plan: &ApiGatewayPlan) -> Result<ApiGatewayState, CloudError> {
        let api_id = format!("api-{}", uuid::Uuid::new_v4().to_string()[..8].to_string());
        let api_state = ApiGatewayState {
            api_id: api_id.clone(),
            api_url: format!(
                "https://{}.execute-api.us-east-1.amazonaws.com/{}",
                api_id, plan.stage
            ),
            stage: plan.stage.clone(),
            resource_id: api_id.clone(),
        };

        let mut apis = self.apis.lock().unwrap();
        apis.insert(api_id, api_state.clone());

        Ok(api_state)
    }

    async fn update_api(
        &self,
        api_id: &str,
        plan: &ApiGatewayPlan,
    ) -> Result<ApiGatewayState, CloudError> {
        let api_state = ApiGatewayState {
            api_id: api_id.to_string(),
            api_url: format!(
                "https://{}.execute-api.us-east-1.amazonaws.com/{}",
                api_id, plan.stage
            ),
            stage: plan.stage.clone(),
            resource_id: api_id.to_string(),
        };

        let mut apis = self.apis.lock().unwrap();
        apis.insert(api_id.to_string(), api_state.clone());

        Ok(api_state)
    }

    async fn delete_api(&self, api_id: &str) -> Result<(), CloudError> {
        let mut apis = self.apis.lock().unwrap();
        apis.remove(api_id);
        Ok(())
    }

    async fn get_api(&self, api_id: &str) -> Result<Option<ApiGatewayState>, CloudError> {
        let apis = self.apis.lock().unwrap();
        Ok(apis.get(api_id).cloned())
    }

    async fn deploy_api(&self, api_id: &str, stage: &str) -> Result<String, CloudError> {
        Ok(format!(
            "https://{}.execute-api.us-east-1.amazonaws.com/{}",
            api_id, stage
        ))
    }

    async fn create_route(&self, _api_id: &str, _route: &ApiRoutePlan) -> Result<(), CloudError> {
        // Mock implementation - just return success
        Ok(())
    }

    async fn delete_route(
        &self,
        _api_id: &str,
        _method: &str,
        _path: &str,
    ) -> Result<(), CloudError> {
        // Mock implementation - just return success
        Ok(())
    }
}

/// Mock AWS SNS/SQS client
pub struct MockAwsMessagingClient {
    topics: std::sync::Mutex<HashMap<String, MessageTopicState>>,
    queues: std::sync::Mutex<HashMap<String, MessageQueueState>>,
}

/// Standalone mock messaging client for testing
pub struct MockMessagingClient {
    inner: MockAwsMessagingClient,
}

impl MockMessagingClient {
    pub fn new() -> Self {
        Self {
            inner: MockAwsMessagingClient::new(),
        }
    }
}

#[async_trait]
impl MessagingClient for MockMessagingClient {
    async fn create_topic(&self, plan: &MessageTopicPlan) -> Result<MessageTopicState, CloudError> {
        self.inner.create_topic(plan).await
    }

    async fn delete_topic(&self, topic_id: &str) -> Result<(), CloudError> {
        self.inner.delete_topic(topic_id).await
    }

    async fn get_topic(&self, topic_id: &str) -> Result<Option<MessageTopicState>, CloudError> {
        self.inner.get_topic(topic_id).await
    }

    async fn list_topics(&self) -> Result<Vec<MessageTopicState>, CloudError> {
        self.inner.list_topics().await
    }

    async fn create_queue(&self, plan: &MessageQueuePlan) -> Result<MessageQueueState, CloudError> {
        self.inner.create_queue(plan).await
    }

    async fn delete_queue(&self, queue_id: &str) -> Result<(), CloudError> {
        self.inner.delete_queue(queue_id).await
    }

    async fn get_queue(&self, queue_id: &str) -> Result<Option<MessageQueueState>, CloudError> {
        self.inner.get_queue(queue_id).await
    }

    async fn list_queues(&self) -> Result<Vec<MessageQueueState>, CloudError> {
        self.inner.list_queues().await
    }

    async fn subscribe_queue_to_topic(
        &self,
        queue_id: &str,
        topic_id: &str,
    ) -> Result<String, CloudError> {
        self.inner
            .subscribe_queue_to_topic(queue_id, topic_id)
            .await
    }

    async fn unsubscribe_queue_from_topic(&self, subscription_id: &str) -> Result<(), CloudError> {
        self.inner
            .unsubscribe_queue_from_topic(subscription_id)
            .await
    }

    async fn publish_message(
        &self,
        topic_id: &str,
        message: &str,
        attributes: Option<HashMap<String, String>>,
    ) -> Result<String, CloudError> {
        self.inner
            .publish_message(topic_id, message, attributes)
            .await
    }
}

impl MockAwsMessagingClient {
    pub fn new() -> Self {
        Self {
            topics: std::sync::Mutex::new(HashMap::new()),
            queues: std::sync::Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl MessagingClient for MockAwsMessagingClient {
    async fn create_topic(&self, plan: &MessageTopicPlan) -> Result<MessageTopicState, CloudError> {
        let mut topic_state = MessageTopicState {
            name: plan.name.clone(),
            resource_id: format!("arn:aws:sns:us-east-1:123456789:{}", plan.name),
            display_name: plan.display_name.clone(),
            dead_letter_queue_arn: None,
        };

        // Create dead letter queue if specified
        if let Some(dlq_name) = &plan.dead_letter_queue_name {
            let dlq_plan = MessageQueuePlan {
                name: dlq_name.clone(),
                topic_name: plan.name.clone(),
                visibility_timeout_seconds: 300, // 5 minutes for DLQ
                message_retention_period: 1209600, // 14 days
                max_receive_count: 1,            // DLQ should not retry
                enable_dead_letter_queue: false, // DLQ itself doesn't need a DLQ
            };

            let dlq_state = self.create_queue(&dlq_plan).await?;
            topic_state.dead_letter_queue_arn = Some(dlq_state.resource_id);
        }

        let mut topics = self.topics.lock().unwrap();
        topics.insert(plan.name.clone(), topic_state.clone());

        Ok(topic_state)
    }

    async fn delete_topic(&self, topic_id: &str) -> Result<(), CloudError> {
        let mut topics = self.topics.lock().unwrap();
        topics.retain(|_, topic| topic.resource_id != topic_id);
        Ok(())
    }

    async fn get_topic(&self, topic_id: &str) -> Result<Option<MessageTopicState>, CloudError> {
        let topics = self.topics.lock().unwrap();
        Ok(topics
            .values()
            .find(|topic| topic.resource_id == topic_id)
            .cloned())
    }

    async fn list_topics(&self) -> Result<Vec<MessageTopicState>, CloudError> {
        let topics = self.topics.lock().unwrap();
        Ok(topics.values().cloned().collect())
    }

    async fn create_queue(&self, plan: &MessageQueuePlan) -> Result<MessageQueueState, CloudError> {
        let queue_state = MessageQueueState {
            name: plan.name.clone(),
            resource_id: format!(
                "https://sqs.us-east-1.amazonaws.com/123456789/{}",
                plan.name
            ),
            topic_subscription: None,
            visibility_timeout_seconds: Some(plan.visibility_timeout_seconds),
            message_retention_period: Some(plan.message_retention_period),
            dead_letter_queue: if plan.enable_dead_letter_queue {
                Some(format!(
                    "https://sqs.us-east-1.amazonaws.com/123456789/{}-dlq",
                    plan.name
                ))
            } else {
                None
            },
        };

        let mut queues = self.queues.lock().unwrap();
        queues.insert(plan.name.clone(), queue_state.clone());

        Ok(queue_state)
    }

    async fn delete_queue(&self, queue_id: &str) -> Result<(), CloudError> {
        let mut queues = self.queues.lock().unwrap();
        queues.retain(|_, queue| queue.resource_id != queue_id);
        Ok(())
    }

    async fn get_queue(&self, queue_id: &str) -> Result<Option<MessageQueueState>, CloudError> {
        let queues = self.queues.lock().unwrap();
        Ok(queues
            .values()
            .find(|queue| queue.resource_id == queue_id)
            .cloned())
    }

    async fn list_queues(&self) -> Result<Vec<MessageQueueState>, CloudError> {
        let queues = self.queues.lock().unwrap();
        Ok(queues.values().cloned().collect())
    }

    async fn subscribe_queue_to_topic(
        &self,
        queue_id: &str,
        topic_id: &str,
    ) -> Result<String, CloudError> {
        let subscription_arn = format!("arn:aws:sns:us-east-1:123456789:{}:subscription", topic_id);

        // Update the queue to include the subscription
        let mut queues = self.queues.lock().unwrap();
        for queue in queues.values_mut() {
            if queue.resource_id == queue_id {
                queue.topic_subscription = Some(subscription_arn.clone());
                break;
            }
        }

        Ok(subscription_arn)
    }

    async fn unsubscribe_queue_from_topic(&self, _subscription_id: &str) -> Result<(), CloudError> {
        // Mock implementation - just return success
        Ok(())
    }

    async fn publish_message(
        &self,
        _topic_id: &str,
        _message: &str,
        _attributes: Option<HashMap<String, String>>,
    ) -> Result<String, CloudError> {
        // Mock implementation - return a fake message ID
        Ok(format!(
            "msg-{}",
            uuid::Uuid::new_v4().to_string()[..8].to_string()
        ))
    }
}

/// Mock AWS IAM client
pub struct MockAwsAccessControlClient {
    roles: std::sync::Mutex<HashMap<String, AccessRoleState>>,
}

impl MockAwsAccessControlClient {
    pub fn new() -> Self {
        Self {
            roles: std::sync::Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl AccessControlClient for MockAwsAccessControlClient {
    async fn create_execution_role(
        &self,
        service_name: &str,
        policies: &[AccessPolicyPlan],
    ) -> Result<String, CloudError> {
        let role_arn = format!(
            "arn:aws:iam::123456789:role/{}-execution-role",
            service_name
        );
        let additional_policies = policies
            .iter()
            .map(|p| format!("arn:aws:iam::123456789:policy/{}", p.name))
            .collect();

        let role_state = AccessRoleState {
            function_execution_role: role_arn.clone(),
            additional_policies,
        };

        let mut roles = self.roles.lock().unwrap();
        roles.insert(role_arn.clone(), role_state);

        Ok(role_arn)
    }

    async fn update_execution_role(
        &self,
        role_arn: &str,
        policies: &[AccessPolicyPlan],
    ) -> Result<(), CloudError> {
        let additional_policies = policies
            .iter()
            .map(|p| format!("arn:aws:iam::123456789:policy/{}", p.name))
            .collect();

        let mut roles = self.roles.lock().unwrap();
        if let Some(role) = roles.get_mut(role_arn) {
            role.additional_policies = additional_policies;
        }

        Ok(())
    }

    async fn delete_execution_role(&self, role_arn: &str) -> Result<(), CloudError> {
        let mut roles = self.roles.lock().unwrap();
        roles.remove(role_arn);
        Ok(())
    }

    async fn get_execution_role(
        &self,
        role_arn: &str,
    ) -> Result<Option<AccessRoleState>, CloudError> {
        let roles = self.roles.lock().unwrap();
        Ok(roles.get(role_arn).cloned())
    }

    async fn create_policy(&self, plan: &AccessPolicyPlan) -> Result<String, CloudError> {
        Ok(format!("arn:aws:iam::123456789:policy/{}", plan.name))
    }

    async fn delete_policy(&self, _policy_arn: &str) -> Result<(), CloudError> {
        Ok(())
    }

    async fn attach_policy_to_role(
        &self,
        _role_arn: &str,
        _policy_arn: &str,
    ) -> Result<(), CloudError> {
        Ok(())
    }

    async fn detach_policy_from_role(
        &self,
        _role_arn: &str,
        _policy_arn: &str,
    ) -> Result<(), CloudError> {
        Ok(())
    }
}
