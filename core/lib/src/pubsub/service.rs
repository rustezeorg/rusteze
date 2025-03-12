use core::fmt;
use std::{collections::HashMap, sync::Arc};

use anyhow::anyhow;
use async_trait::async_trait;
use aws_lambda_events::chrono;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{Mutex, broadcast};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SnsRecords {
    pub records: Vec<PubSubMessage>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SnsRecord {
    #[serde(rename = "type")]
    pub record_type: String,
    pub message_id: String,
    pub topic_arn: String,
    pub subject: String,
    pub message: String,
    pub timestamp: String,
    pub signature_version: String,
    pub signature: String,
    #[serde(rename(serialize = "SigningCertURL"))]
    pub signing_cert_url: String,
    #[serde(rename(serialize = "UnsubscribeURL"))]
    pub unsubscribe_url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PubSubMessage {
    pub event_version: String,
    pub event_subscription_arn: String,
    pub event_source: String,
    #[serde(rename(serialize = "Sns"))]
    pub sns: SnsRecord,
}

#[derive(Debug, Clone)]
pub struct Topic {
    pub name: String,
    pub subscribers: Vec<String>,
    pub sender: broadcast::Sender<SnsRecords>,
}

#[derive(Debug)]
pub struct Queue {
    name: String,
    topic_subscription: Option<String>,
    messages: Arc<Mutex<Vec<SnsRecords>>>,
    pub binary_name: String,
    pub receiver: Option<broadcast::Receiver<SnsRecords>>,
}

#[derive(Debug)]
pub struct PubSubService {
    pub topics: Mutex<HashMap<String, Topic>>,
    pub queues: Mutex<HashMap<String, Queue>>,
}

pub struct RustezeResult {}

#[derive(Debug, Clone)]
pub enum RustezeError {
    PubSubError(String),
}

impl fmt::Display for RustezeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RustezeError::PubSubError(msg) => write!(f, "Error creating PubSub service: {:?}", msg),
        }
    }
}

impl std::error::Error for RustezeError {}

impl PubSubService {
    pub fn new() -> Self {
        Self {
            topics: Mutex::new(HashMap::new()),
            queues: Mutex::new(HashMap::new()),
        }
    }

    pub async fn create_topic(
        &self,
        topic_name: &str,
        size: Option<usize>,
    ) -> anyhow::Result<Topic> {
        let mut topics = self.topics.lock().await;

        if topics.contains_key(topic_name) {
            return Err(anyhow!("Topic {} already exists", &topic_name));
        }

        let (sender, _) = broadcast::channel(size.unwrap_or(1000));

        let topic = Topic {
            name: topic_name.to_string(),
            subscribers: Vec::new(),
            sender,
        };

        println!("Created topic: {:?}", &topic);

        let topic_clone = topic.clone();
        topics.insert(topic_name.to_string(), topic);

        Ok(topic_clone)
    }

    pub async fn create_queue(
        &self,
        queue_name: &str,
        topic_name: Option<&str>,
        binary_name: &str,
    ) -> anyhow::Result<()> {
        let mut queues = self.queues.lock().await;

        if queues.contains_key(queue_name) {
            return Ok(());
        }

        let receiver = if let Some(topic) = topic_name {
            let topics = self.topics.lock().await;

            if let Some(m_topic) = topics.get(topic) {
                Some(m_topic.sender.subscribe())
            } else {
                return Err(RustezeError::PubSubError(format!(
                    "Unable to find topic: {:?}",
                    &topic_name
                ))
                .into());
            }
        } else {
            None
        };

        let queue = Queue {
            name: queue_name.to_string(),
            topic_subscription: topic_name.map(|s| s.to_string()),
            messages: Arc::new(Mutex::new(Vec::new())),
            binary_name: binary_name.to_string(),
            receiver,
        };

        queues.insert(queue_name.to_string(), queue);

        // Update the topic subscribers
        if let Some(topic) = topic_name {
            let mut topics = self.topics.lock().await;

            if let Some(m_topic) = topics.get_mut(topic) {
                m_topic.subscribers.push(queue_name.to_string());
            }

            println!(
                "Created queue: {} subscribed to {:?}",
                queue_name, topic_name
            );
        }

        Ok(())
    }

    pub async fn get_topic_subscribers(&self, topic_name: &str) -> Option<Vec<(String, String)>> {
        let topics = self.topics.lock().await;
        let queues = self.queues.lock().await;
        let mut subscriber_info = Vec::new();

        let subs = match topics.get(topic_name) {
            Some(t) => &t.subscribers,
            None => return None,
        };

        println!("subs: {:?}", &subs);

        for sub in subs {
            if let Some(queue) = queues.get(sub) {
                subscriber_info.push((queue.name.clone(), queue.binary_name.clone()));
            }
        }

        println!("Getting topic subs: {:?}", &subscriber_info);
        Some(subscriber_info)
    }
}

#[async_trait]
pub trait Publisher: Send + Sync {
    async fn publish(&self, topic: &str, message: &serde_json::Value) -> anyhow::Result<()>;
}

pub struct LocalPublisher {
    endpoint: String,
}

#[async_trait::async_trait]
impl Publisher for LocalPublisher {
    async fn publish(&self, topic: &str, message: &serde_json::Value) -> anyhow::Result<()> {
        let url = format!("{}/rusteze/publish/{}", self.endpoint, topic);
        println!("Sending to url: {}", &url);
        reqwest::Client::new()
            .post(&url)
            .body(message.to_string())
            .send()
            .await?;
        Ok(())
    }
}

// AWS SNS publisher
pub struct SnsPublisher {
    client: aws_sdk_sns::Client,
}

#[async_trait::async_trait]
impl Publisher for SnsPublisher {
    async fn publish(&self, topic: &str, message: &serde_json::Value) -> anyhow::Result<()> {
        self.client
            .publish()
            .topic_arn(topic)
            .message(message.to_string())
            .send()
            .await?;
        Ok(())
    }
}

pub struct PublisherBuilder;

impl PublisherBuilder {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn build(self) -> anyhow::Result<Box<dyn Publisher + Send + Sync>> {
        let env = std::env::var("RUSTEZE_LOCAL").unwrap_or("false".to_string());

        println!("is RUSTEZE_LOCAL: {}", &env);
        match env.as_str() {
            "true" => {
                // @todo - what happens if we run on another port?
                let endpoint = "http://localhost:3000".to_string();
                Ok(Box::new(LocalPublisher { endpoint }))
            }
            "false" => {
                let config = aws_config::load_from_env().await;
                let client = aws_sdk_sns::Client::new(&config);
                Ok(Box::new(SnsPublisher { client }))
            }
            _ => Err(anyhow!("Unable to find RUSTEZE_LOCAL")),
        }
    }
}
