use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Enhanced rusteze.toml configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RustezeConfig {
    pub service_name: String,
    pub deployment: DeploymentConfig,
    pub lambda: Option<LambdaConfig>,
    pub pubsub: Option<PubSubConfig>,
    pub environment: Option<HashMap<String, String>>,
    /// Environment-specific configuration overrides
    pub environments: Option<HashMap<String, EnvironmentOverride>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeploymentConfig {
    pub provider: String,
    pub region: String,
    #[serde(rename = "type")]
    pub deployment_type: Option<String>,
    pub stage: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LambdaConfig {
    pub memory_size: Option<u32>,
    pub timeout: Option<u32>,
    pub runtime: Option<String>,
    pub environment_variables: Option<HashMap<String, String>>,
}

impl Default for LambdaConfig {
    fn default() -> Self {
        Self {
            memory_size: Some(256),
            timeout: Some(30),
            runtime: Some("provided.al2".to_string()),
            environment_variables: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PubSubConfig {
    /// Message retention period in days (1-365)
    pub message_retention_days: Option<u32>,
    /// Visibility timeout for SQS messages in seconds (1-43200)
    pub visibility_timeout_seconds: Option<u32>,
    /// Maximum number of times a message can be received before moving to DLQ (1-1000)
    pub max_receive_count: Option<u32>,
    /// Enable dead letter queues for failed messages
    pub enable_dead_letter_queues: Option<bool>,
    /// Prefix for SNS topic names
    pub topic_prefix: Option<String>,
    /// Prefix for SQS queue names
    pub queue_prefix: Option<String>,
    /// Message delivery delay in seconds (0-900)
    pub delivery_delay_seconds: Option<u32>,
    /// Maximum message size in bytes (1024-262144)
    pub max_message_size_bytes: Option<u32>,
    /// Enable message encryption at rest
    pub enable_encryption: Option<bool>,
    /// KMS key ID for encryption (if encryption is enabled)
    pub kms_key_id: Option<String>,
    /// Enable FIFO queues for ordered message processing
    pub enable_fifo: Option<bool>,
    /// Content-based deduplication for FIFO queues
    pub content_based_deduplication: Option<bool>,
    /// Message group ID for FIFO queues
    pub message_group_id: Option<String>,
}

impl Default for PubSubConfig {
    fn default() -> Self {
        Self {
            message_retention_days: Some(14),
            visibility_timeout_seconds: Some(30),
            max_receive_count: Some(3),
            enable_dead_letter_queues: Some(true),
            topic_prefix: None,
            queue_prefix: None,
            delivery_delay_seconds: Some(0),
            max_message_size_bytes: Some(262144), // 256KB
            enable_encryption: Some(false),
            kms_key_id: None,
            enable_fifo: Some(false),
            content_based_deduplication: Some(false),
            message_group_id: None,
        }
    }
}

/// Environment-specific configuration overrides
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnvironmentOverride {
    pub deployment: Option<DeploymentConfig>,
    pub lambda: Option<LambdaConfig>,
    pub pubsub: Option<PubSubConfig>,
    pub environment: Option<HashMap<String, String>>,
}

impl RustezeConfig {
    /// Load configuration from rusteze.toml file
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(path).map_err(|e| ConfigError::FileRead(e.to_string()))?;

        let config: RustezeConfig =
            toml::from_str(&content).map_err(|e| ConfigError::ParseError(e.to_string()))?;

        config.validate()?;
        Ok(config)
    }

    /// Load configuration from current directory
    pub fn load() -> Result<Self, ConfigError> {
        Self::load_from_file("rusteze.toml")
    }

    /// Validate configuration values
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.service_name.is_empty() {
            return Err(ConfigError::Validation(
                "service_name cannot be empty".to_string(),
            ));
        }

        if self.deployment.provider.is_empty() {
            return Err(ConfigError::Validation(
                "deployment.provider cannot be empty".to_string(),
            ));
        }

        if self.deployment.region.is_empty() {
            return Err(ConfigError::Validation(
                "deployment.region cannot be empty".to_string(),
            ));
        }

        // Validate lambda configuration if present
        if let Some(lambda_config) = &self.lambda {
            if let Some(memory_size) = lambda_config.memory_size {
                if memory_size < 128 || memory_size > 10240 {
                    return Err(ConfigError::Validation(
                        "lambda.memory_size must be between 128 and 10240 MB".to_string(),
                    ));
                }
            }

            if let Some(timeout) = lambda_config.timeout {
                if timeout == 0 || timeout > 900 {
                    return Err(ConfigError::Validation(
                        "lambda.timeout must be between 1 and 900 seconds".to_string(),
                    ));
                }
            }
        }

        // Validate pubsub configuration if present
        if let Some(pubsub_config) = &self.pubsub {
            self.validate_pubsub_config(pubsub_config)?;
        }

        // Validate environment overrides if present
        if let Some(environments) = &self.environments {
            for (env_name, env_override) in environments {
                if env_name.is_empty() {
                    return Err(ConfigError::Validation(
                        "Environment name cannot be empty".to_string(),
                    ));
                }

                // Validate deployment config in override
                if let Some(deployment) = &env_override.deployment {
                    if deployment.provider.is_empty() {
                        return Err(ConfigError::Validation(format!(
                            "environments.{}.deployment.provider cannot be empty",
                            env_name
                        )));
                    }
                    if deployment.region.is_empty() {
                        return Err(ConfigError::Validation(format!(
                            "environments.{}.deployment.region cannot be empty",
                            env_name
                        )));
                    }
                }

                // Validate lambda config in override
                if let Some(lambda_config) = &env_override.lambda {
                    self.validate_lambda_config(lambda_config, Some(env_name))?;
                }

                // Validate pubsub config in override
                if let Some(pubsub_config) = &env_override.pubsub {
                    self.validate_pubsub_config_with_context(pubsub_config, Some(env_name))?;
                }
            }
        }

        Ok(())
    }

    /// Get effective lambda configuration with defaults
    pub fn get_lambda_config(&self) -> LambdaConfig {
        self.lambda.clone().unwrap_or_else(|| LambdaConfig {
            memory_size: Some(256),
            timeout: Some(30),
            runtime: Some("provided.al2".to_string()),
            environment_variables: None,
        })
    }

    /// Get effective pubsub configuration with defaults
    pub fn get_pubsub_config(&self) -> PubSubConfig {
        self.pubsub.clone().unwrap_or_default()
    }

    /// Get deployment stage with default
    pub fn get_stage(&self) -> String {
        self.deployment
            .stage
            .clone()
            .unwrap_or_else(|| "prod".to_string())
    }

    /// Apply environment-specific overrides to the configuration
    pub fn with_environment(&self, environment: &str) -> Result<Self, ConfigError> {
        let mut config = self.clone();

        if let Some(environments) = &self.environments {
            if let Some(env_override) = environments.get(environment) {
                // Apply deployment overrides
                if let Some(deployment_override) = &env_override.deployment {
                    if !deployment_override.provider.is_empty() {
                        config.deployment.provider = deployment_override.provider.clone();
                    }
                    if !deployment_override.region.is_empty() {
                        config.deployment.region = deployment_override.region.clone();
                    }
                    if let Some(deployment_type) = &deployment_override.deployment_type {
                        config.deployment.deployment_type = Some(deployment_type.clone());
                    }
                    if let Some(stage) = &deployment_override.stage {
                        config.deployment.stage = Some(stage.clone());
                    }
                }

                // Apply lambda overrides
                if let Some(lambda_override) = &env_override.lambda {
                    let mut lambda_config = config.lambda.unwrap_or_default();
                    if let Some(memory_size) = lambda_override.memory_size {
                        lambda_config.memory_size = Some(memory_size);
                    }
                    if let Some(timeout) = lambda_override.timeout {
                        lambda_config.timeout = Some(timeout);
                    }
                    if let Some(runtime) = &lambda_override.runtime {
                        lambda_config.runtime = Some(runtime.clone());
                    }
                    if let Some(env_vars) = &lambda_override.environment_variables {
                        lambda_config.environment_variables = Some(env_vars.clone());
                    }
                    config.lambda = Some(lambda_config);
                }

                // Apply pubsub overrides
                if let Some(pubsub_override) = &env_override.pubsub {
                    let mut pubsub_config = config.pubsub.unwrap_or_default();
                    if let Some(retention_days) = pubsub_override.message_retention_days {
                        pubsub_config.message_retention_days = Some(retention_days);
                    }
                    if let Some(visibility_timeout) = pubsub_override.visibility_timeout_seconds {
                        pubsub_config.visibility_timeout_seconds = Some(visibility_timeout);
                    }
                    if let Some(max_receive_count) = pubsub_override.max_receive_count {
                        pubsub_config.max_receive_count = Some(max_receive_count);
                    }
                    if let Some(enable_dlq) = pubsub_override.enable_dead_letter_queues {
                        pubsub_config.enable_dead_letter_queues = Some(enable_dlq);
                    }
                    if let Some(topic_prefix) = &pubsub_override.topic_prefix {
                        pubsub_config.topic_prefix = Some(topic_prefix.clone());
                    }
                    if let Some(queue_prefix) = &pubsub_override.queue_prefix {
                        pubsub_config.queue_prefix = Some(queue_prefix.clone());
                    }
                    if let Some(delivery_delay) = pubsub_override.delivery_delay_seconds {
                        pubsub_config.delivery_delay_seconds = Some(delivery_delay);
                    }
                    if let Some(max_message_size) = pubsub_override.max_message_size_bytes {
                        pubsub_config.max_message_size_bytes = Some(max_message_size);
                    }
                    if let Some(enable_encryption) = pubsub_override.enable_encryption {
                        pubsub_config.enable_encryption = Some(enable_encryption);
                    }
                    if let Some(kms_key_id) = &pubsub_override.kms_key_id {
                        pubsub_config.kms_key_id = Some(kms_key_id.clone());
                    }
                    if let Some(enable_fifo) = pubsub_override.enable_fifo {
                        pubsub_config.enable_fifo = Some(enable_fifo);
                    }
                    if let Some(content_based_dedup) = pubsub_override.content_based_deduplication {
                        pubsub_config.content_based_deduplication = Some(content_based_dedup);
                    }
                    if let Some(message_group_id) = &pubsub_override.message_group_id {
                        pubsub_config.message_group_id = Some(message_group_id.clone());
                    }
                    config.pubsub = Some(pubsub_config);
                }

                // Apply environment variable overrides
                if let Some(env_vars) = &env_override.environment {
                    let mut combined_env = config.environment.unwrap_or_default();
                    combined_env.extend(env_vars.clone());
                    config.environment = Some(combined_env);
                }
            }
        }

        config.validate()?;
        Ok(config)
    }

    /// Validate lambda configuration with optional context
    fn validate_lambda_config(
        &self,
        lambda_config: &LambdaConfig,
        context: Option<&str>,
    ) -> Result<(), ConfigError> {
        let prefix = context
            .map(|c| format!("environments.{}.lambda.", c))
            .unwrap_or_else(|| "lambda.".to_string());

        if let Some(memory_size) = lambda_config.memory_size {
            if memory_size < 128 || memory_size > 10240 {
                return Err(ConfigError::Validation(format!(
                    "{}memory_size must be between 128 and 10240 MB",
                    prefix
                )));
            }
        }

        if let Some(timeout) = lambda_config.timeout {
            if timeout == 0 || timeout > 900 {
                return Err(ConfigError::Validation(format!(
                    "{}timeout must be between 1 and 900 seconds",
                    prefix
                )));
            }
        }

        Ok(())
    }

    /// Validate pubsub configuration
    fn validate_pubsub_config(&self, pubsub_config: &PubSubConfig) -> Result<(), ConfigError> {
        self.validate_pubsub_config_with_context(pubsub_config, None)
    }

    /// Validate pubsub configuration with optional context
    fn validate_pubsub_config_with_context(
        &self,
        pubsub_config: &PubSubConfig,
        context: Option<&str>,
    ) -> Result<(), ConfigError> {
        let prefix = context
            .map(|c| format!("environments.{}.pubsub.", c))
            .unwrap_or_else(|| "pubsub.".to_string());

        if let Some(retention_days) = pubsub_config.message_retention_days {
            if retention_days == 0 || retention_days > 365 {
                return Err(ConfigError::Validation(format!(
                    "{}message_retention_days must be between 1 and 365 days",
                    prefix
                )));
            }
        }

        if let Some(visibility_timeout) = pubsub_config.visibility_timeout_seconds {
            if visibility_timeout == 0 || visibility_timeout > 43200 {
                return Err(ConfigError::Validation(format!(
                    "{}visibility_timeout_seconds must be between 1 and 43200 seconds (12 hours)",
                    prefix
                )));
            }
        }

        if let Some(max_receive_count) = pubsub_config.max_receive_count {
            if max_receive_count == 0 || max_receive_count > 1000 {
                return Err(ConfigError::Validation(format!(
                    "{}max_receive_count must be between 1 and 1000",
                    prefix
                )));
            }
        }

        if let Some(delivery_delay) = pubsub_config.delivery_delay_seconds {
            if delivery_delay > 900 {
                return Err(ConfigError::Validation(format!(
                    "{}delivery_delay_seconds must be between 0 and 900 seconds (15 minutes)",
                    prefix
                )));
            }
        }

        if let Some(max_message_size) = pubsub_config.max_message_size_bytes {
            if max_message_size < 1024 || max_message_size > 262144 {
                return Err(ConfigError::Validation(format!(
                    "{}max_message_size_bytes must be between 1024 and 262144 bytes (1KB - 256KB)",
                    prefix
                )));
            }
        }

        // Validate FIFO-specific settings
        if let Some(enable_fifo) = pubsub_config.enable_fifo {
            if enable_fifo {
                if let Some(content_based_dedup) = pubsub_config.content_based_deduplication {
                    if content_based_dedup && pubsub_config.message_group_id.is_some() {
                        return Err(ConfigError::Validation(format!(
                            "{}message_group_id should not be set when content_based_deduplication is enabled",
                            prefix
                        )));
                    }
                }
            } else {
                // If FIFO is disabled, FIFO-specific settings should not be set
                if pubsub_config.content_based_deduplication.is_some() {
                    return Err(ConfigError::Validation(format!(
                        "{}content_based_deduplication can only be set when enable_fifo is true",
                        prefix
                    )));
                }
                if pubsub_config.message_group_id.is_some() {
                    return Err(ConfigError::Validation(format!(
                        "{}message_group_id can only be set when enable_fifo is true",
                        prefix
                    )));
                }
            }
        }

        // Validate encryption settings
        if let Some(enable_encryption) = pubsub_config.enable_encryption {
            if enable_encryption && pubsub_config.kms_key_id.is_none() {
                return Err(ConfigError::Validation(format!(
                    "{}kms_key_id must be provided when enable_encryption is true",
                    prefix
                )));
            }
        }

        Ok(())
    }

    /// Generate a hash of the configuration for change detection
    pub fn generate_config_hash(&self) -> String {
        let mut hasher = Sha256::new();

        // Serialize the configuration to a consistent format for hashing
        let config_json = serde_json::to_string(self).unwrap_or_default();
        hasher.update(config_json.as_bytes());

        format!("{:x}", hasher.finalize())
    }

    /// Compare this configuration with another to detect changes
    pub fn detect_changes(&self, other: &RustezeConfig) -> ConfigChanges {
        let mut changes = ConfigChanges::new();

        // Check deployment changes
        if self.deployment != other.deployment {
            changes.deployment_changed = true;
        }

        // Check lambda changes
        if self.lambda != other.lambda {
            changes.lambda_changed = true;
        }

        // Check pubsub changes
        if self.pubsub != other.pubsub {
            changes.pubsub_changed = true;
        }

        // Check environment variable changes
        if self.environment != other.environment {
            changes.environment_changed = true;
        }

        // Check environment overrides changes
        if self.environments != other.environments {
            changes.environments_changed = true;
        }

        changes
    }

    /// Check if configuration is compatible with a previous version
    pub fn is_backward_compatible(
        &self,
        previous_config: &RustezeConfig,
    ) -> Result<(), ConfigCompatibilityError> {
        // Check for breaking changes that would require resource recreation

        // Service name changes are not allowed
        if self.service_name != previous_config.service_name {
            return Err(ConfigCompatibilityError::ServiceNameChanged {
                old: previous_config.service_name.clone(),
                new: self.service_name.clone(),
            });
        }

        // Provider changes are not allowed
        if self.deployment.provider != previous_config.deployment.provider {
            return Err(ConfigCompatibilityError::ProviderChanged {
                old: previous_config.deployment.provider.clone(),
                new: self.deployment.provider.clone(),
            });
        }

        // Check for FIFO queue changes (can't change FIFO setting after creation)
        if let (Some(current_pubsub), Some(previous_pubsub)) =
            (&self.pubsub, &previous_config.pubsub)
        {
            if current_pubsub.enable_fifo != previous_pubsub.enable_fifo {
                return Err(ConfigCompatibilityError::FifoSettingChanged);
            }
        }

        Ok(())
    }
}

/// Configuration change detection results
#[derive(Debug, Clone, Default)]
pub struct ConfigChanges {
    pub deployment_changed: bool,
    pub lambda_changed: bool,
    pub pubsub_changed: bool,
    pub environment_changed: bool,
    pub environments_changed: bool,
}

impl ConfigChanges {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn has_changes(&self) -> bool {
        self.deployment_changed
            || self.lambda_changed
            || self.pubsub_changed
            || self.environment_changed
            || self.environments_changed
    }

    pub fn requires_resource_update(&self) -> bool {
        // Lambda and pubsub changes require resource updates
        self.lambda_changed || self.pubsub_changed
    }

    pub fn requires_redeployment(&self) -> bool {
        // Deployment changes require full redeployment
        self.deployment_changed
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigCompatibilityError {
    #[error("Service name cannot be changed: '{old}' -> '{new}'")]
    ServiceNameChanged { old: String, new: String },

    #[error("Cloud provider cannot be changed: '{old}' -> '{new}'")]
    ProviderChanged { old: String, new: String },

    #[error("FIFO queue setting cannot be changed after creation")]
    FifoSettingChanged,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Failed to read configuration file: {0}")]
    FileRead(String),

    #[error("Failed to parse configuration: {0}")]
    ParseError(String),

    #[error("Configuration validation error: {0}")]
    Validation(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_validation() {
        let config = RustezeConfig {
            service_name: "test-service".to_string(),
            deployment: DeploymentConfig {
                provider: "aws".to_string(),
                region: "us-east-1".to_string(),
                deployment_type: Some("lambda".to_string()),
                stage: Some("prod".to_string()),
            },
            lambda: None,
            pubsub: None,
            environment: None,
            environments: None,
        };

        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_empty_service_name() {
        let config = RustezeConfig {
            service_name: "".to_string(),
            deployment: DeploymentConfig {
                provider: "aws".to_string(),
                region: "us-east-1".to_string(),
                deployment_type: Some("lambda".to_string()),
                stage: Some("prod".to_string()),
            },
            lambda: None,
            pubsub: None,
            environment: None,
            environments: None,
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_lambda_config_defaults() {
        let config = RustezeConfig {
            service_name: "test-service".to_string(),
            deployment: DeploymentConfig {
                provider: "aws".to_string(),
                region: "us-east-1".to_string(),
                deployment_type: Some("lambda".to_string()),
                stage: Some("prod".to_string()),
            },
            lambda: None,
            pubsub: None,
            environment: None,
            environments: None,
        };

        let lambda_config = config.get_lambda_config();
        assert_eq!(lambda_config.memory_size, Some(256));
        assert_eq!(lambda_config.timeout, Some(30));
        assert_eq!(lambda_config.runtime, Some("provided.al2".to_string()));
    }

    #[test]
    fn test_pubsub_config_validation() {
        let config = RustezeConfig {
            service_name: "test-service".to_string(),
            deployment: DeploymentConfig {
                provider: "aws".to_string(),
                region: "us-east-1".to_string(),
                deployment_type: Some("lambda".to_string()),
                stage: Some("prod".to_string()),
            },
            lambda: None,
            pubsub: Some(PubSubConfig {
                message_retention_days: Some(400), // Invalid: > 365
                visibility_timeout_seconds: Some(30),
                max_receive_count: Some(3),
                enable_dead_letter_queues: Some(true),
                topic_prefix: None,
                queue_prefix: None,
                delivery_delay_seconds: Some(0),
                max_message_size_bytes: Some(262144),
                enable_encryption: Some(false),
                kms_key_id: None,
                enable_fifo: Some(false),
                content_based_deduplication: Some(false),
                message_group_id: None,
            }),
            environment: None,
            environments: None,
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_environment_override() {
        let mut environments = HashMap::new();
        environments.insert(
            "staging".to_string(),
            EnvironmentOverride {
                deployment: Some(DeploymentConfig {
                    provider: "aws".to_string(),
                    region: "us-west-2".to_string(),
                    deployment_type: Some("lambda".to_string()),
                    stage: Some("staging".to_string()),
                }),
                lambda: Some(LambdaConfig {
                    memory_size: Some(512),
                    timeout: Some(60),
                    runtime: Some("provided.al2".to_string()),
                    environment_variables: None,
                }),
                pubsub: None,
                environment: None,
            },
        );

        let config = RustezeConfig {
            service_name: "test-service".to_string(),
            deployment: DeploymentConfig {
                provider: "aws".to_string(),
                region: "us-east-1".to_string(),
                deployment_type: Some("lambda".to_string()),
                stage: Some("prod".to_string()),
            },
            lambda: Some(LambdaConfig {
                memory_size: Some(256),
                timeout: Some(30),
                runtime: Some("provided.al2".to_string()),
                environment_variables: None,
            }),
            pubsub: None,
            environment: None,
            environments: Some(environments),
        };

        let staging_config = config.with_environment("staging").unwrap();
        assert_eq!(staging_config.deployment.region, "us-west-2");
        assert_eq!(staging_config.deployment.stage, Some("staging".to_string()));
        assert_eq!(staging_config.lambda.unwrap().memory_size, Some(512));
    }

    #[test]
    fn test_fifo_validation() {
        let config = RustezeConfig {
            service_name: "test-service".to_string(),
            deployment: DeploymentConfig {
                provider: "aws".to_string(),
                region: "us-east-1".to_string(),
                deployment_type: Some("lambda".to_string()),
                stage: Some("prod".to_string()),
            },
            lambda: None,
            pubsub: Some(PubSubConfig {
                message_retention_days: Some(14),
                visibility_timeout_seconds: Some(30),
                max_receive_count: Some(3),
                enable_dead_letter_queues: Some(true),
                topic_prefix: None,
                queue_prefix: None,
                delivery_delay_seconds: Some(0),
                max_message_size_bytes: Some(262144),
                enable_encryption: Some(false),
                kms_key_id: None,
                enable_fifo: Some(false),
                content_based_deduplication: Some(true), // Invalid: FIFO disabled but dedup enabled
                message_group_id: None,
            }),
            environment: None,
            environments: None,
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_encryption_validation() {
        let config = RustezeConfig {
            service_name: "test-service".to_string(),
            deployment: DeploymentConfig {
                provider: "aws".to_string(),
                region: "us-east-1".to_string(),
                deployment_type: Some("lambda".to_string()),
                stage: Some("prod".to_string()),
            },
            lambda: None,
            pubsub: Some(PubSubConfig {
                message_retention_days: Some(14),
                visibility_timeout_seconds: Some(30),
                max_receive_count: Some(3),
                enable_dead_letter_queues: Some(true),
                topic_prefix: None,
                queue_prefix: None,
                delivery_delay_seconds: Some(0),
                max_message_size_bytes: Some(262144),
                enable_encryption: Some(true), // Encryption enabled but no KMS key
                kms_key_id: None,
                enable_fifo: Some(false),
                content_based_deduplication: Some(false),
                message_group_id: None,
            }),
            environment: None,
            environments: None,
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_hash_generation() {
        let config = RustezeConfig {
            service_name: "test-service".to_string(),
            deployment: DeploymentConfig {
                provider: "aws".to_string(),
                region: "us-east-1".to_string(),
                deployment_type: Some("lambda".to_string()),
                stage: Some("prod".to_string()),
            },
            lambda: None,
            pubsub: None,
            environment: None,
            environments: None,
        };

        let hash1 = config.generate_config_hash();
        let hash2 = config.generate_config_hash();
        assert_eq!(hash1, hash2);

        // Different config should produce different hash
        let mut config2 = config.clone();
        config2.service_name = "different-service".to_string();
        let hash3 = config2.generate_config_hash();
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_config_change_detection() {
        let config1 = RustezeConfig {
            service_name: "test-service".to_string(),
            deployment: DeploymentConfig {
                provider: "aws".to_string(),
                region: "us-east-1".to_string(),
                deployment_type: Some("lambda".to_string()),
                stage: Some("prod".to_string()),
            },
            lambda: Some(LambdaConfig {
                memory_size: Some(256),
                timeout: Some(30),
                runtime: Some("provided.al2".to_string()),
                environment_variables: None,
            }),
            pubsub: None,
            environment: None,
            environments: None,
        };

        let mut config2 = config1.clone();
        config2.lambda.as_mut().unwrap().memory_size = Some(512);

        let changes = config1.detect_changes(&config2);
        assert!(changes.lambda_changed);
        assert!(!changes.deployment_changed);
        assert!(!changes.pubsub_changed);
        assert!(changes.has_changes());
        assert!(changes.requires_resource_update());
    }

    #[test]
    fn test_backward_compatibility() {
        let config1 = RustezeConfig {
            service_name: "test-service".to_string(),
            deployment: DeploymentConfig {
                provider: "aws".to_string(),
                region: "us-east-1".to_string(),
                deployment_type: Some("lambda".to_string()),
                stage: Some("prod".to_string()),
            },
            lambda: None,
            pubsub: None,
            environment: None,
            environments: None,
        };

        // Compatible change (same service name and provider)
        let mut config2 = config1.clone();
        config2.deployment.region = "us-west-2".to_string();
        assert!(config2.is_backward_compatible(&config1).is_ok());

        // Incompatible change (different service name)
        let mut config3 = config1.clone();
        config3.service_name = "different-service".to_string();
        assert!(config3.is_backward_compatible(&config1).is_err());

        // Incompatible change (different provider)
        let mut config4 = config1.clone();
        config4.deployment.provider = "gcp".to_string();
        assert!(config4.is_backward_compatible(&config1).is_err());
    }
}
