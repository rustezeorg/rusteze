use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Structured logging for deployment operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentLogEntry {
    pub timestamp: DateTime<Utc>,
    pub operation: String,
    pub resource_type: String,
    pub resource_name: String,
    pub status: OperationStatus,
    pub duration_ms: Option<u64>,
    pub details: HashMap<String, String>,
    pub error: Option<String>,
}

/// Status of a deployment operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationStatus {
    Started,
    InProgress,
    Completed,
    Failed,
    Skipped,
}

impl std::fmt::Display for OperationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OperationStatus::Started => write!(f, "STARTED"),
            OperationStatus::InProgress => write!(f, "IN_PROGRESS"),
            OperationStatus::Completed => write!(f, "COMPLETED"),
            OperationStatus::Failed => write!(f, "FAILED"),
            OperationStatus::Skipped => write!(f, "SKIPPED"),
        }
    }
}

/// Audit trail entry for resource changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: DateTime<Utc>,
    pub deployment_id: String,
    pub action: AuditAction,
    pub resource_type: String,
    pub resource_name: String,
    pub resource_id: Option<String>,
    pub previous_state: Option<String>,
    pub new_state: Option<String>,
    pub metadata: HashMap<String, String>,
}

/// Types of audit actions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuditAction {
    Create,
    Update,
    Delete,
    Reconcile,
    Drift,
}

impl std::fmt::Display for AuditAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditAction::Create => write!(f, "CREATE"),
            AuditAction::Update => write!(f, "UPDATE"),
            AuditAction::Delete => write!(f, "DELETE"),
            AuditAction::Reconcile => write!(f, "RECONCILE"),
            AuditAction::Drift => write!(f, "DRIFT"),
        }
    }
}

/// Deployment summary for reporting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentSummaryReport {
    pub deployment_id: String,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub status: DeploymentSummaryStatus,
    pub total_resources: usize,
    pub resources_created: usize,
    pub resources_updated: usize,
    pub resources_deleted: usize,
    pub resources_failed: usize,
    pub resource_details: Vec<ResourceSummary>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Overall deployment status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeploymentSummaryStatus {
    InProgress,
    Completed,
    Failed,
    PartiallyCompleted,
}

impl std::fmt::Display for DeploymentSummaryStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeploymentSummaryStatus::InProgress => write!(f, "IN_PROGRESS"),
            DeploymentSummaryStatus::Completed => write!(f, "COMPLETED"),
            DeploymentSummaryStatus::Failed => write!(f, "FAILED"),
            DeploymentSummaryStatus::PartiallyCompleted => write!(f, "PARTIALLY_COMPLETED"),
        }
    }
}

/// Summary of individual resource operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSummary {
    pub resource_type: String,
    pub resource_name: String,
    pub action: AuditAction,
    pub status: OperationStatus,
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
}

/// Logger for deployment operations and audit trail
pub struct DeploymentLogger {
    log_file_path: PathBuf,
    audit_file_path: PathBuf,
    current_deployment_id: Option<String>,
    current_summary: Option<DeploymentSummaryReport>,
}

impl DeploymentLogger {
    /// Create a new deployment logger
    pub fn new<P: AsRef<Path>>(log_dir: P) -> std::io::Result<Self> {
        let log_dir = log_dir.as_ref();
        std::fs::create_dir_all(log_dir)?;

        let log_file_path = log_dir.join("deployment.log");
        let audit_file_path = log_dir.join("audit.log");

        Ok(Self {
            log_file_path,
            audit_file_path,
            current_deployment_id: None,
            current_summary: None,
        })
    }

    /// Create a default deployment logger in .rusteze-logs directory
    pub fn default() -> std::io::Result<Self> {
        Self::new(".rusteze-logs")
    }

    /// Start a new deployment session
    pub fn start_deployment(&mut self, deployment_id: String) -> std::io::Result<()> {
        self.current_deployment_id = Some(deployment_id.clone());
        self.current_summary = Some(DeploymentSummaryReport {
            deployment_id: deployment_id.clone(),
            start_time: Utc::now(),
            end_time: None,
            status: DeploymentSummaryStatus::InProgress,
            total_resources: 0,
            resources_created: 0,
            resources_updated: 0,
            resources_deleted: 0,
            resources_failed: 0,
            resource_details: Vec::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
        });

        self.log_operation(
            "deployment_start",
            "deployment",
            &deployment_id,
            OperationStatus::Started,
            None,
            HashMap::new(),
            None,
        )
    }

    /// End the current deployment session
    pub fn end_deployment(&mut self, status: DeploymentSummaryStatus) -> std::io::Result<()> {
        let (
            deployment_id,
            total_resources,
            resources_created,
            resources_updated,
            resources_deleted,
            resources_failed,
        ) = if let Some(ref mut summary) = self.current_summary {
            summary.end_time = Some(Utc::now());
            summary.status = status.clone();

            (
                summary.deployment_id.clone(),
                summary.total_resources,
                summary.resources_created,
                summary.resources_updated,
                summary.resources_deleted,
                summary.resources_failed,
            )
        } else {
            return Ok(());
        };

        // Log deployment completion
        let mut details = HashMap::new();
        details.insert("total_resources".to_string(), total_resources.to_string());
        details.insert(
            "resources_created".to_string(),
            resources_created.to_string(),
        );
        details.insert(
            "resources_updated".to_string(),
            resources_updated.to_string(),
        );
        details.insert(
            "resources_deleted".to_string(),
            resources_deleted.to_string(),
        );
        details.insert("resources_failed".to_string(), resources_failed.to_string());

        let operation_status = match status {
            DeploymentSummaryStatus::Completed => OperationStatus::Completed,
            DeploymentSummaryStatus::Failed => OperationStatus::Failed,
            DeploymentSummaryStatus::PartiallyCompleted => OperationStatus::Completed,
            DeploymentSummaryStatus::InProgress => OperationStatus::InProgress,
        };

        self.log_operation(
            "deployment_end",
            "deployment",
            &deployment_id,
            operation_status,
            None,
            details,
            None,
        )?;

        Ok(())
    }

    /// Log a deployment operation
    pub fn log_operation(
        &self,
        operation: &str,
        resource_type: &str,
        resource_name: &str,
        status: OperationStatus,
        duration_ms: Option<u64>,
        details: HashMap<String, String>,
        error: Option<String>,
    ) -> std::io::Result<()> {
        let entry = DeploymentLogEntry {
            timestamp: Utc::now(),
            operation: operation.to_string(),
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
            status,
            duration_ms,
            details,
            error,
        };

        self.write_log_entry(&entry)
    }

    /// Log an audit entry for resource changes
    pub fn log_audit(
        &self,
        action: AuditAction,
        resource_type: &str,
        resource_name: &str,
        resource_id: Option<String>,
        previous_state: Option<String>,
        new_state: Option<String>,
        metadata: HashMap<String, String>,
    ) -> std::io::Result<()> {
        let deployment_id = self
            .current_deployment_id
            .as_ref()
            .unwrap_or(&"unknown".to_string())
            .clone();

        let entry = AuditEntry {
            timestamp: Utc::now(),
            deployment_id,
            action: action.clone(),
            resource_type: resource_type.to_string(),
            resource_name: resource_name.to_string(),
            resource_id,
            previous_state,
            new_state,
            metadata,
        };

        self.write_audit_entry(&entry)?;

        // Note: Summary statistics are updated via add_resource_summary method

        Ok(())
    }

    /// Add a resource to the current deployment summary
    pub fn add_resource_summary(
        &mut self,
        resource_type: &str,
        resource_name: &str,
        action: AuditAction,
        status: OperationStatus,
        duration_ms: Option<u64>,
        error: Option<String>,
    ) {
        if let Some(ref mut summary) = self.current_summary {
            summary.total_resources += 1;

            match action {
                AuditAction::Create => summary.resources_created += 1,
                AuditAction::Update => summary.resources_updated += 1,
                AuditAction::Delete => summary.resources_deleted += 1,
                _ => {}
            }

            if matches!(status, OperationStatus::Failed) {
                summary.resources_failed += 1;
            }

            summary.resource_details.push(ResourceSummary {
                resource_type: resource_type.to_string(),
                resource_name: resource_name.to_string(),
                action,
                status,
                duration_ms,
                error,
            });
        }
    }

    /// Add an error to the current deployment summary
    pub fn add_error(&mut self, error: String) {
        if let Some(ref mut summary) = self.current_summary {
            summary.errors.push(error);
        }
    }

    /// Add a warning to the current deployment summary
    pub fn add_warning(&mut self, warning: String) {
        if let Some(ref mut summary) = self.current_summary {
            summary.warnings.push(warning);
        }
    }

    /// Get the current deployment summary
    pub fn get_summary(&self) -> Option<&DeploymentSummaryReport> {
        self.current_summary.as_ref()
    }

    /// Write a log entry to the log file
    fn write_log_entry(&self, entry: &DeploymentLogEntry) -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_file_path)?;

        let json_line = serde_json::to_string(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        writeln!(file, "{}", json_line)?;
        file.flush()?;

        // Also print to console for immediate feedback
        println!(
            "[{}] {} {} {} - {}",
            entry.timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
            entry.status,
            entry.resource_type.to_uppercase(),
            entry.resource_name,
            entry.operation
        );

        if let Some(ref error) = entry.error {
            println!("  Error: {}", error);
        }

        Ok(())
    }

    /// Write an audit entry to the audit file
    fn write_audit_entry(&self, entry: &AuditEntry) -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.audit_file_path)?;

        let json_line = serde_json::to_string(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        writeln!(file, "{}", json_line)?;
        file.flush()?;

        Ok(())
    }

    /// Print deployment summary to console
    pub fn print_summary(&self) {
        if let Some(summary) = &self.current_summary {
            println!("\n📊 Deployment Summary");
            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            println!("Deployment ID: {}", summary.deployment_id);
            println!("Status: {}", summary.status);
            println!(
                "Start Time: {}",
                summary.start_time.format("%Y-%m-%d %H:%M:%S UTC")
            );

            if let Some(end_time) = summary.end_time {
                println!("End Time: {}", end_time.format("%Y-%m-%d %H:%M:%S UTC"));
                let duration = end_time.signed_duration_since(summary.start_time);
                println!(
                    "Duration: {:.2}s",
                    duration.num_milliseconds() as f64 / 1000.0
                );
            }

            println!("\n📈 Resource Statistics:");
            println!("  Total Resources: {}", summary.total_resources);
            println!("  Created: {}", summary.resources_created);
            println!("  Updated: {}", summary.resources_updated);
            println!("  Deleted: {}", summary.resources_deleted);
            println!("  Failed: {}", summary.resources_failed);

            if !summary.resource_details.is_empty() {
                println!("\n📋 Resource Details:");
                for resource in &summary.resource_details {
                    let status_icon = match resource.status {
                        OperationStatus::Completed => "✅",
                        OperationStatus::Failed => "❌",
                        OperationStatus::InProgress => "⏳",
                        OperationStatus::Started => "🔄",
                        OperationStatus::Skipped => "⏭️",
                    };

                    println!(
                        "  {} {} {} ({})",
                        status_icon,
                        resource.resource_type.to_uppercase(),
                        resource.resource_name,
                        resource.action
                    );

                    if let Some(duration) = resource.duration_ms {
                        println!("    Duration: {}ms", duration);
                    }

                    if let Some(ref error) = resource.error {
                        println!("    Error: {}", error);
                    }
                }
            }

            if !summary.errors.is_empty() {
                println!("\n❌ Errors:");
                for error in &summary.errors {
                    println!("  - {}", error);
                }
            }

            if !summary.warnings.is_empty() {
                println!("\n⚠️  Warnings:");
                for warning in &summary.warnings {
                    println!("  - {}", warning);
                }
            }

            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        }
    }
}

/// Helper trait for timing operations
pub trait TimedOperation {
    fn timed<F, R>(
        &mut self,
        operation: &str,
        resource_type: &str,
        resource_name: &str,
        f: F,
    ) -> Result<R, crate::error::RustezeError>
    where
        F: FnOnce() -> Result<R, crate::error::RustezeError>;
}

impl TimedOperation for DeploymentLogger {
    fn timed<F, R>(
        &mut self,
        operation: &str,
        resource_type: &str,
        resource_name: &str,
        f: F,
    ) -> Result<R, crate::error::RustezeError>
    where
        F: FnOnce() -> Result<R, crate::error::RustezeError>,
    {
        let start_time = std::time::Instant::now();

        // Log operation start
        let _ = self.log_operation(
            operation,
            resource_type,
            resource_name,
            OperationStatus::Started,
            None,
            HashMap::new(),
            None,
        );

        let result = f();
        let duration = start_time.elapsed();
        let duration_ms = duration.as_millis() as u64;

        match &result {
            Ok(_) => {
                let _ = self.log_operation(
                    operation,
                    resource_type,
                    resource_name,
                    OperationStatus::Completed,
                    Some(duration_ms),
                    HashMap::new(),
                    None,
                );
            }
            Err(error) => {
                let _ = self.log_operation(
                    operation,
                    resource_type,
                    resource_name,
                    OperationStatus::Failed,
                    Some(duration_ms),
                    HashMap::new(),
                    Some(error.to_string()),
                );
            }
        }

        result
    }
}
