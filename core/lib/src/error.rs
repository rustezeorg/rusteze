use crate::cloud::{CloudError, PlanningError, StateError};
use crate::config::ConfigError;
use std::fmt;

/// Error context for providing additional information about errors
#[derive(Debug, Clone)]
pub struct ErrorContext {
    pub operation: String,
    pub resource_name: Option<String>,
    pub resource_type: Option<String>,
    pub additional_info: Vec<String>,
}

impl ErrorContext {
    pub fn new(operation: impl Into<String>) -> Self {
        Self {
            operation: operation.into(),
            resource_name: None,
            resource_type: None,
            additional_info: Vec::new(),
        }
    }

    pub fn with_resource(
        mut self,
        name: impl Into<String>,
        resource_type: impl Into<String>,
    ) -> Self {
        self.resource_name = Some(name.into());
        self.resource_type = Some(resource_type.into());
        self
    }

    pub fn with_info(mut self, info: impl Into<String>) -> Self {
        self.additional_info.push(info.into());
        self
    }
}

impl fmt::Display for ErrorContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Operation: {}", self.operation)?;
        if let Some(ref name) = self.resource_name {
            write!(f, ", Resource: {}", name)?;
        }
        if let Some(ref resource_type) = self.resource_type {
            write!(f, " ({})", resource_type)?;
        }
        if !self.additional_info.is_empty() {
            write!(f, ", Info: {}", self.additional_info.join(", "))?;
        }
        Ok(())
    }
}

/// Main error type for the Rusteze framework with enhanced context and categorization
#[derive(Debug, thiserror::Error)]
pub enum RustezeError {
    #[error("Configuration error: {message}")]
    Config {
        #[source]
        source: Option<ConfigError>,
        message: String,
        context: Option<ErrorContext>,
    },

    #[error("Cloud deployment error: {message}")]
    Cloud {
        #[source]
        source: Option<CloudError>,
        message: String,
        context: Option<ErrorContext>,
    },

    #[error("State management error: {message}")]
    State {
        #[source]
        source: Option<StateError>,
        message: String,
        context: Option<ErrorContext>,
    },

    #[error("Resource planning error: {message}")]
    Planning {
        #[source]
        source: Option<PlanningError>,
        message: String,
        context: Option<ErrorContext>,
    },

    #[error("Local server error: {message}")]
    LocalServer {
        message: String,
        context: Option<ErrorContext>,
    },

    #[error("Pub/sub error: {message}")]
    PubSub {
        message: String,
        context: Option<ErrorContext>,
    },

    #[error("Macro processing error: {message}")]
    MacroProcessing {
        message: String,
        context: Option<ErrorContext>,
    },

    #[error("Build error: {message}")]
    Build {
        message: String,
        context: Option<ErrorContext>,
    },

    #[error("IO error: {message}")]
    Io {
        #[source]
        source: std::io::Error,
        message: String,
        context: Option<ErrorContext>,
    },

    #[error("Serialization error: {message}")]
    Serialization {
        message: String,
        context: Option<ErrorContext>,
    },

    #[error("Authentication error: {message}")]
    Authentication {
        message: String,
        context: Option<ErrorContext>,
    },

    #[error("Permission error: {message}")]
    Permission {
        message: String,
        context: Option<ErrorContext>,
    },

    #[error("Network error: {message}")]
    Network {
        message: String,
        context: Option<ErrorContext>,
    },

    #[error("Timeout error: {message}")]
    Timeout {
        message: String,
        context: Option<ErrorContext>,
    },

    #[error("Resource conflict error: {message}")]
    ResourceConflict {
        message: String,
        context: Option<ErrorContext>,
    },

    #[error("Validation error: {message}")]
    Validation {
        message: String,
        context: Option<ErrorContext>,
    },
}

impl RustezeError {
    /// Create a new configuration error with context
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config {
            source: None,
            message: message.into(),
            context: None,
        }
    }

    /// Create a new cloud error with context
    pub fn cloud(message: impl Into<String>) -> Self {
        Self::Cloud {
            source: None,
            message: message.into(),
            context: None,
        }
    }

    /// Create a new state error with context
    pub fn state(message: impl Into<String>) -> Self {
        Self::State {
            source: None,
            message: message.into(),
            context: None,
        }
    }

    /// Create a new local server error with context
    pub fn local_server(message: impl Into<String>) -> Self {
        Self::LocalServer {
            message: message.into(),
            context: None,
        }
    }

    /// Create a new pub/sub error with context
    pub fn pubsub(message: impl Into<String>) -> Self {
        Self::PubSub {
            message: message.into(),
            context: None,
        }
    }

    /// Create a new network error with context
    pub fn network(message: impl Into<String>) -> Self {
        Self::Network {
            message: message.into(),
            context: None,
        }
    }

    /// Create a new timeout error with context
    pub fn timeout(message: impl Into<String>) -> Self {
        Self::Timeout {
            message: message.into(),
            context: None,
        }
    }

    /// Create a new authentication error with context
    pub fn authentication(message: impl Into<String>) -> Self {
        Self::Authentication {
            message: message.into(),
            context: None,
        }
    }

    /// Create a new permission error with context
    pub fn permission(message: impl Into<String>) -> Self {
        Self::Permission {
            message: message.into(),
            context: None,
        }
    }

    /// Create a new resource conflict error with context
    pub fn resource_conflict(message: impl Into<String>) -> Self {
        Self::ResourceConflict {
            message: message.into(),
            context: None,
        }
    }

    /// Create a new validation error with context
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation {
            message: message.into(),
            context: None,
        }
    }

    /// Create a new serialization error with context
    pub fn serialization(message: impl Into<String>) -> Self {
        Self::Serialization {
            message: message.into(),
            context: None,
        }
    }

    /// Add context to an existing error
    pub fn with_context(mut self, context: ErrorContext) -> Self {
        match &mut self {
            RustezeError::Config { context: ctx, .. } => *ctx = Some(context),
            RustezeError::Cloud { context: ctx, .. } => *ctx = Some(context),
            RustezeError::State { context: ctx, .. } => *ctx = Some(context),
            RustezeError::Planning { context: ctx, .. } => *ctx = Some(context),
            RustezeError::LocalServer { context: ctx, .. } => *ctx = Some(context),
            RustezeError::PubSub { context: ctx, .. } => *ctx = Some(context),
            RustezeError::MacroProcessing { context: ctx, .. } => *ctx = Some(context),
            RustezeError::Build { context: ctx, .. } => *ctx = Some(context),
            RustezeError::Io { context: ctx, .. } => *ctx = Some(context),
            RustezeError::Serialization { context: ctx, .. } => *ctx = Some(context),
            RustezeError::Authentication { context: ctx, .. } => *ctx = Some(context),
            RustezeError::Permission { context: ctx, .. } => *ctx = Some(context),
            RustezeError::Network { context: ctx, .. } => *ctx = Some(context),
            RustezeError::Timeout { context: ctx, .. } => *ctx = Some(context),
            RustezeError::ResourceConflict { context: ctx, .. } => *ctx = Some(context),
            RustezeError::Validation { context: ctx, .. } => *ctx = Some(context),
        }
        self
    }

    /// Check if the error is retryable
    pub fn is_retryable(&self) -> bool {
        match self {
            RustezeError::Cloud {
                source: Some(cloud_error),
                ..
            } => cloud_error.is_retryable(),
            RustezeError::Network { .. } => true,
            RustezeError::Timeout { .. } => true,
            RustezeError::Io { .. } => true,
            _ => false,
        }
    }

    /// Get the severity level of the error
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            RustezeError::Config { .. } => ErrorSeverity::High,
            RustezeError::Cloud { .. } => ErrorSeverity::High,
            RustezeError::State { .. } => ErrorSeverity::High,
            RustezeError::Planning { .. } => ErrorSeverity::Medium,
            RustezeError::LocalServer { .. } => ErrorSeverity::Low,
            RustezeError::PubSub { .. } => ErrorSeverity::Medium,
            RustezeError::MacroProcessing { .. } => ErrorSeverity::High,
            RustezeError::Build { .. } => ErrorSeverity::High,
            RustezeError::Io { .. } => ErrorSeverity::Medium,
            RustezeError::Serialization { .. } => ErrorSeverity::Medium,
            RustezeError::Authentication { .. } => ErrorSeverity::Critical,
            RustezeError::Permission { .. } => ErrorSeverity::High,
            RustezeError::Network { .. } => ErrorSeverity::Medium,
            RustezeError::Timeout { .. } => ErrorSeverity::Medium,
            RustezeError::ResourceConflict { .. } => ErrorSeverity::High,
            RustezeError::Validation { .. } => ErrorSeverity::Medium,
        }
    }

    /// Get suggested remediation steps for the error
    pub fn remediation_steps(&self) -> Vec<String> {
        match self {
            RustezeError::Config { .. } => vec![
                "Check your rusteze.toml configuration file".to_string(),
                "Ensure all required fields are present".to_string(),
                "Validate configuration values against documentation".to_string(),
                "Run 'cargo rusteze validate-config' to check configuration".to_string(),
            ],
            RustezeError::Cloud {
                source: Some(cloud_error),
                ..
            } => cloud_error.remediation_steps(),
            RustezeError::Cloud { .. } => vec![
                "Check your AWS credentials and permissions".to_string(),
                "Verify your AWS region configuration".to_string(),
                "Ensure your AWS account has sufficient quotas".to_string(),
                "Check AWS service status for outages".to_string(),
            ],
            RustezeError::State { .. } => vec![
                "Check if .rusteze-state.toml file exists and is readable".to_string(),
                "Try running 'cargo rusteze reconcile' to fix state issues".to_string(),
                "Backup and recreate state file if corrupted".to_string(),
                "Ensure proper file permissions for state file".to_string(),
            ],
            RustezeError::Planning { .. } => vec![
                "Review your route and macro configurations".to_string(),
                "Check for naming conflicts in resources".to_string(),
                "Ensure all dependencies are properly defined".to_string(),
                "Validate resource naming conventions".to_string(),
            ],
            RustezeError::LocalServer { .. } => vec![
                "Check if the port is already in use".to_string(),
                "Ensure your code compiles successfully".to_string(),
                "Review local server configuration".to_string(),
                "Try a different port number".to_string(),
            ],
            RustezeError::PubSub { .. } => vec![
                "Check pub/sub configuration in rusteze.toml".to_string(),
                "Verify topic and queue naming conventions".to_string(),
                "Ensure proper macro usage for publishers and subscribers".to_string(),
                "Check message format and serialization".to_string(),
            ],
            RustezeError::MacroProcessing { .. } => vec![
                "Check macro syntax and parameters".to_string(),
                "Ensure all required macro attributes are present".to_string(),
                "Review function signatures for macro compatibility".to_string(),
                "Update to latest rusteze version if needed".to_string(),
            ],
            RustezeError::Build { .. } => vec![
                "Run 'cargo check' to identify compilation errors".to_string(),
                "Ensure all dependencies are properly configured".to_string(),
                "Check for missing features or incompatible versions".to_string(),
                "Clear target directory and rebuild".to_string(),
            ],
            RustezeError::Io { .. } => vec![
                "Check file and directory permissions".to_string(),
                "Ensure sufficient disk space".to_string(),
                "Verify file paths are correct".to_string(),
                "Check if files are locked by other processes".to_string(),
            ],
            RustezeError::Serialization { .. } => vec![
                "Check data format and structure".to_string(),
                "Ensure compatibility between versions".to_string(),
                "Validate input data against expected schema".to_string(),
                "Check for special characters or encoding issues".to_string(),
            ],
            RustezeError::Authentication { .. } => vec![
                "Check your AWS credentials configuration".to_string(),
                "Ensure AWS CLI is properly configured".to_string(),
                "Verify IAM user or role permissions".to_string(),
                "Check if MFA is required and properly configured".to_string(),
            ],
            RustezeError::Permission { .. } => vec![
                "Review IAM policies and permissions".to_string(),
                "Ensure your user/role has necessary permissions".to_string(),
                "Check resource-based policies".to_string(),
                "Verify service-linked roles are created".to_string(),
            ],
            RustezeError::Network { .. } => vec![
                "Check your internet connection".to_string(),
                "Verify firewall and proxy settings".to_string(),
                "Check if AWS services are accessible".to_string(),
                "Try again after a brief delay".to_string(),
            ],
            RustezeError::Timeout { .. } => vec![
                "Increase timeout values in configuration".to_string(),
                "Check network connectivity".to_string(),
                "Verify AWS service performance".to_string(),
                "Retry the operation".to_string(),
            ],
            RustezeError::ResourceConflict { .. } => vec![
                "Check for existing resources with the same name".to_string(),
                "Use different resource names".to_string(),
                "Delete conflicting resources if safe to do so".to_string(),
                "Review resource naming strategy".to_string(),
            ],
            RustezeError::Validation { .. } => vec![
                "Check input data format and values".to_string(),
                "Ensure all required fields are provided".to_string(),
                "Validate against schema or constraints".to_string(),
                "Review error message for specific validation failures".to_string(),
            ],
        }
    }

    /// Get error category for logging and metrics
    pub fn category(&self) -> &'static str {
        match self {
            RustezeError::Config { .. } => "configuration",
            RustezeError::Cloud { .. } => "cloud",
            RustezeError::State { .. } => "state",
            RustezeError::Planning { .. } => "planning",
            RustezeError::LocalServer { .. } => "local_server",
            RustezeError::PubSub { .. } => "pubsub",
            RustezeError::MacroProcessing { .. } => "macro",
            RustezeError::Build { .. } => "build",
            RustezeError::Io { .. } => "io",
            RustezeError::Serialization { .. } => "serialization",
            RustezeError::Authentication { .. } => "authentication",
            RustezeError::Permission { .. } => "permission",
            RustezeError::Network { .. } => "network",
            RustezeError::Timeout { .. } => "timeout",
            RustezeError::ResourceConflict { .. } => "resource_conflict",
            RustezeError::Validation { .. } => "validation",
        }
    }

    /// Get the error context if available
    pub fn context(&self) -> Option<&ErrorContext> {
        match self {
            RustezeError::Config { context, .. } => context.as_ref(),
            RustezeError::Cloud { context, .. } => context.as_ref(),
            RustezeError::State { context, .. } => context.as_ref(),
            RustezeError::Planning { context, .. } => context.as_ref(),
            RustezeError::LocalServer { context, .. } => context.as_ref(),
            RustezeError::PubSub { context, .. } => context.as_ref(),
            RustezeError::MacroProcessing { context, .. } => context.as_ref(),
            RustezeError::Build { context, .. } => context.as_ref(),
            RustezeError::Io { context, .. } => context.as_ref(),
            RustezeError::Serialization { context, .. } => context.as_ref(),
            RustezeError::Authentication { context, .. } => context.as_ref(),
            RustezeError::Permission { context, .. } => context.as_ref(),
            RustezeError::Network { context, .. } => context.as_ref(),
            RustezeError::Timeout { context, .. } => context.as_ref(),
            RustezeError::ResourceConflict { context, .. } => context.as_ref(),
            RustezeError::Validation { context, .. } => context.as_ref(),
        }
    }

    /// Create a formatted error report with context and remediation steps
    pub fn error_report(&self) -> String {
        let mut report = format!("Error: {}\n", self);

        if let Some(context) = self.context() {
            report.push_str(&format!("Context: {}\n", context));
        }

        report.push_str(&format!("Category: {}\n", self.category()));
        report.push_str(&format!("Severity: {:?}\n", self.severity()));
        report.push_str(&format!("Retryable: {}\n", self.is_retryable()));

        let steps = self.remediation_steps();
        if !steps.is_empty() {
            report.push_str("\nRemediation Steps:\n");
            for (i, step) in steps.iter().enumerate() {
                report.push_str(&format!("  {}. {}\n", i + 1, step));
            }
        }

        report
    }
}

/// Error severity levels for prioritization and alerting
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ErrorSeverity {
    Low,
    Medium,
    High,
    Critical,
}

impl fmt::Display for ErrorSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorSeverity::Low => write!(f, "Low"),
            ErrorSeverity::Medium => write!(f, "Medium"),
            ErrorSeverity::High => write!(f, "High"),
            ErrorSeverity::Critical => write!(f, "Critical"),
        }
    }
}

// Implement From traits for backwards compatibility and easy conversion
impl From<ConfigError> for RustezeError {
    fn from(err: ConfigError) -> Self {
        RustezeError::Config {
            source: Some(err),
            message: "Configuration error occurred".to_string(),
            context: None,
        }
    }
}

impl From<std::io::Error> for RustezeError {
    fn from(err: std::io::Error) -> Self {
        RustezeError::Io {
            source: err,
            message: "IO operation failed".to_string(),
            context: None,
        }
    }
}

impl From<CloudError> for RustezeError {
    fn from(err: CloudError) -> Self {
        RustezeError::Cloud {
            source: Some(err),
            message: "Cloud operation failed".to_string(),
            context: None,
        }
    }
}

impl From<StateError> for RustezeError {
    fn from(err: StateError) -> Self {
        RustezeError::State {
            source: Some(err),
            message: "State operation failed".to_string(),
            context: None,
        }
    }
}

impl From<PlanningError> for RustezeError {
    fn from(err: PlanningError) -> Self {
        RustezeError::Planning {
            source: Some(err),
            message: "Resource planning failed".to_string(),
            context: None,
        }
    }
}

/// Result type alias for Rusteze operations
pub type RustezeResult<T> = Result<T, RustezeError>;
