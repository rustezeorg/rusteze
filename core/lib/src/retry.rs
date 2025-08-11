use crate::error::RustezeError;
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// Configuration for retry behavior
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    /// Initial delay between retries
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Multiplier for exponential backoff
    pub backoff_multiplier: f64,
    /// Maximum total time to spend retrying
    pub max_total_time: Duration,
    /// Jitter factor to add randomness to delays (0.0 to 1.0)
    pub jitter_factor: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            max_total_time: Duration::from_secs(300), // 5 minutes
            jitter_factor: 0.1,
        }
    }
}

impl RetryConfig {
    /// Create a new retry configuration for cloud operations
    pub fn for_cloud_operations() -> Self {
        Self {
            max_attempts: 5,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(60),
            backoff_multiplier: 2.0,
            max_total_time: Duration::from_secs(600), // 10 minutes
            jitter_factor: 0.2,
        }
    }

    /// Create a new retry configuration for network operations
    pub fn for_network_operations() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(10),
            backoff_multiplier: 1.5,
            max_total_time: Duration::from_secs(60),
            jitter_factor: 0.1,
        }
    }

    /// Create a new retry configuration for state operations
    pub fn for_state_operations() -> Self {
        Self {
            max_attempts: 2,
            initial_delay: Duration::from_millis(50),
            max_delay: Duration::from_secs(5),
            backoff_multiplier: 2.0,
            max_total_time: Duration::from_secs(30),
            jitter_factor: 0.05,
        }
    }
}

/// Circuit breaker states
#[derive(Debug, Clone, PartialEq)]
pub enum CircuitBreakerState {
    /// Circuit is closed, requests are allowed through
    Closed,
    /// Circuit is open, requests are rejected immediately
    Open,
    /// Circuit is half-open, allowing limited requests to test if service has recovered
    HalfOpen,
}

/// Configuration for circuit breaker behavior
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening the circuit
    pub failure_threshold: u32,
    /// Time to wait before transitioning from Open to HalfOpen
    pub recovery_timeout: Duration,
    /// Number of successful requests needed in HalfOpen state to close the circuit
    pub success_threshold: u32,
    /// Maximum number of requests allowed in HalfOpen state
    pub half_open_max_requests: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            recovery_timeout: Duration::from_secs(60),
            success_threshold: 3,
            half_open_max_requests: 5,
        }
    }
}

/// Circuit breaker implementation for handling repeated failures
#[derive(Debug)]
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    state: CircuitBreakerState,
    failure_count: u32,
    success_count: u32,
    last_failure_time: Option<Instant>,
    half_open_requests: u32,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with the given configuration
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            state: CircuitBreakerState::Closed,
            failure_count: 0,
            success_count: 0,
            last_failure_time: None,
            half_open_requests: 0,
        }
    }

    /// Create a new circuit breaker with default configuration
    pub fn default() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }

    /// Check if a request should be allowed through the circuit breaker
    pub fn should_allow_request(&mut self) -> bool {
        match self.state {
            CircuitBreakerState::Closed => true,
            CircuitBreakerState::Open => {
                if let Some(last_failure) = self.last_failure_time {
                    if last_failure.elapsed() >= self.config.recovery_timeout {
                        self.transition_to_half_open();
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitBreakerState::HalfOpen => {
                if self.half_open_requests < self.config.half_open_max_requests {
                    self.half_open_requests += 1;
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Record a successful operation
    pub fn record_success(&mut self) {
        match self.state {
            CircuitBreakerState::Closed => {
                self.failure_count = 0;
            }
            CircuitBreakerState::HalfOpen => {
                self.success_count += 1;
                if self.success_count >= self.config.success_threshold {
                    self.transition_to_closed();
                }
            }
            CircuitBreakerState::Open => {
                // This shouldn't happen, but reset if it does
                self.transition_to_closed();
            }
        }
    }

    /// Record a failed operation
    pub fn record_failure(&mut self) {
        self.failure_count += 1;
        self.last_failure_time = Some(Instant::now());

        match self.state {
            CircuitBreakerState::Closed => {
                if self.failure_count >= self.config.failure_threshold {
                    self.transition_to_open();
                }
            }
            CircuitBreakerState::HalfOpen => {
                self.transition_to_open();
            }
            CircuitBreakerState::Open => {
                // Already open, just update the failure time
            }
        }
    }

    /// Get the current state of the circuit breaker
    pub fn state(&self) -> &CircuitBreakerState {
        &self.state
    }

    /// Get the current failure count
    pub fn failure_count(&self) -> u32 {
        self.failure_count
    }

    /// Transition to closed state
    fn transition_to_closed(&mut self) {
        self.state = CircuitBreakerState::Closed;
        self.failure_count = 0;
        self.success_count = 0;
        self.half_open_requests = 0;
    }

    /// Transition to open state
    fn transition_to_open(&mut self) {
        self.state = CircuitBreakerState::Open;
        self.success_count = 0;
        self.half_open_requests = 0;
    }

    /// Transition to half-open state
    fn transition_to_half_open(&mut self) {
        self.state = CircuitBreakerState::HalfOpen;
        self.success_count = 0;
        self.half_open_requests = 0;
    }
}

/// Retry executor that implements exponential backoff with jitter
pub struct RetryExecutor {
    config: RetryConfig,
    circuit_breaker: Option<CircuitBreaker>,
}

impl RetryExecutor {
    /// Create a new retry executor with the given configuration
    pub fn new(config: RetryConfig) -> Self {
        Self {
            config,
            circuit_breaker: None,
        }
    }

    /// Create a new retry executor with circuit breaker
    pub fn with_circuit_breaker(config: RetryConfig, circuit_breaker: CircuitBreaker) -> Self {
        Self {
            config,
            circuit_breaker: Some(circuit_breaker),
        }
    }

    /// Execute an operation with retry logic and optional circuit breaker
    pub async fn execute<F, Fut, T>(&mut self, operation: F) -> Result<T, RustezeError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, RustezeError>>,
    {
        let start_time = Instant::now();
        let mut attempt = 0;
        let mut delay = self.config.initial_delay;

        loop {
            // Check circuit breaker if present
            if let Some(ref mut circuit_breaker) = self.circuit_breaker {
                if !circuit_breaker.should_allow_request() {
                    return Err(RustezeError::network(
                        "Circuit breaker is open, request rejected",
                    ));
                }
            }

            attempt += 1;

            // Check if we've exceeded the maximum total time
            if start_time.elapsed() > self.config.max_total_time {
                return Err(RustezeError::timeout(format!(
                    "Operation timed out after {} attempts over {:?}",
                    attempt - 1,
                    start_time.elapsed()
                )));
            }

            // Execute the operation
            match operation().await {
                Ok(result) => {
                    // Record success in circuit breaker if present
                    if let Some(ref mut circuit_breaker) = self.circuit_breaker {
                        circuit_breaker.record_success();
                    }
                    return Ok(result);
                }
                Err(error) => {
                    // Check if the error is retryable
                    if !error.is_retryable() || attempt >= self.config.max_attempts {
                        // Record failure in circuit breaker if present
                        if let Some(ref mut circuit_breaker) = self.circuit_breaker {
                            circuit_breaker.record_failure();
                        }
                        return Err(error);
                    }

                    // Record failure in circuit breaker if present
                    if let Some(ref mut circuit_breaker) = self.circuit_breaker {
                        circuit_breaker.record_failure();
                    }

                    // Calculate delay with jitter
                    let jitter = if self.config.jitter_factor > 0.0 {
                        let jitter_amount = delay.as_millis() as f64 * self.config.jitter_factor;
                        let random_jitter = (rand::random::<f64>() - 0.5) * 2.0 * jitter_amount;
                        Duration::from_millis(random_jitter.abs() as u64)
                    } else {
                        Duration::from_millis(0)
                    };

                    let actual_delay = delay + jitter;
                    let capped_delay = std::cmp::min(actual_delay, self.config.max_delay);

                    // Log retry attempt
                    println!(
                        "⚠️  Operation failed (attempt {}/{}), retrying in {:?}. Error: {}",
                        attempt, self.config.max_attempts, capped_delay, error
                    );

                    // Wait before retrying
                    sleep(capped_delay).await;

                    // Calculate next delay using exponential backoff
                    delay = Duration::from_millis(
                        (delay.as_millis() as f64 * self.config.backoff_multiplier) as u64,
                    );
                }
            }
        }
    }

    /// Execute an operation with a timeout
    pub async fn execute_with_timeout<F, Fut, T>(
        &mut self,
        operation: F,
        timeout: Duration,
    ) -> Result<T, RustezeError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, RustezeError>>,
    {
        match tokio::time::timeout(timeout, self.execute(operation)).await {
            Ok(result) => result,
            Err(_) => Err(RustezeError::timeout(format!(
                "Operation timed out after {:?}",
                timeout
            ))),
        }
    }

    /// Get the current circuit breaker state if present
    pub fn circuit_breaker_state(&self) -> Option<&CircuitBreakerState> {
        self.circuit_breaker.as_ref().map(|cb| cb.state())
    }

    /// Get the current failure count from circuit breaker if present
    pub fn circuit_breaker_failure_count(&self) -> Option<u32> {
        self.circuit_breaker.as_ref().map(|cb| cb.failure_count())
    }
}

/// Convenience function to retry an operation with default configuration
pub async fn retry_operation<F, Fut, T>(operation: F) -> Result<T, RustezeError>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, RustezeError>>,
{
    let mut executor = RetryExecutor::new(RetryConfig::default());
    executor.execute(operation).await
}

/// Convenience function to retry a cloud operation with appropriate configuration
pub async fn retry_cloud_operation<F, Fut, T>(operation: F) -> Result<T, RustezeError>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, RustezeError>>,
{
    let mut executor = RetryExecutor::new(RetryConfig::for_cloud_operations());
    executor.execute(operation).await
}

/// Convenience function to retry a network operation with appropriate configuration
pub async fn retry_network_operation<F, Fut, T>(operation: F) -> Result<T, RustezeError>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, RustezeError>>,
{
    let mut executor = RetryExecutor::new(RetryConfig::for_network_operations());
    executor.execute(operation).await
}

/// Convenience function to retry a state operation with appropriate configuration
pub async fn retry_state_operation<F, Fut, T>(operation: F) -> Result<T, RustezeError>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, RustezeError>>,
{
    let mut executor = RetryExecutor::new(RetryConfig::for_state_operations());
    executor.execute(operation).await
}

/// Timeout wrapper for operations
pub async fn with_timeout<F, Fut, T>(operation: F, timeout: Duration) -> Result<T, RustezeError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, RustezeError>>,
{
    match tokio::time::timeout(timeout, operation()).await {
        Ok(result) => result,
        Err(_) => Err(RustezeError::timeout(format!(
            "Operation timed out after {:?}",
            timeout
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test]
    async fn test_retry_success_on_first_attempt() {
        let mut executor = RetryExecutor::new(RetryConfig::default());
        let result = executor
            .execute(|| async { Ok::<i32, RustezeError>(42) })
            .await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_retry_success_after_failures() {
        let attempt_count = Arc::new(AtomicU32::new(0));
        let attempt_count_clone = attempt_count.clone();

        let mut executor = RetryExecutor::new(RetryConfig {
            max_attempts: 3,
            initial_delay: Duration::from_millis(10),
            ..RetryConfig::default()
        });

        let result = executor
            .execute(move || {
                let count = attempt_count_clone.fetch_add(1, Ordering::SeqCst);
                async move {
                    if count < 2 {
                        Err(RustezeError::network("Temporary failure"))
                    } else {
                        Ok(42)
                    }
                }
            })
            .await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempt_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_max_attempts_exceeded() {
        let mut executor = RetryExecutor::new(RetryConfig {
            max_attempts: 2,
            initial_delay: Duration::from_millis(10),
            ..RetryConfig::default()
        });

        let result = executor
            .execute(|| async { Err::<i32, RustezeError>(RustezeError::network("Always fails")) })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_circuit_breaker_opens_after_failures() {
        let circuit_breaker = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 2,
            recovery_timeout: Duration::from_millis(100),
            ..CircuitBreakerConfig::default()
        });

        let mut executor = RetryExecutor::with_circuit_breaker(
            RetryConfig {
                max_attempts: 1,
                ..RetryConfig::default()
            },
            circuit_breaker,
        );

        // First failure
        let _ = executor
            .execute(|| async { Err::<i32, RustezeError>(RustezeError::network("Failure 1")) })
            .await;

        // Second failure should open the circuit
        let _ = executor
            .execute(|| async { Err::<i32, RustezeError>(RustezeError::network("Failure 2")) })
            .await;

        // Third attempt should be rejected by circuit breaker
        let result = executor
            .execute(|| async { Ok::<i32, RustezeError>(42) })
            .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Circuit breaker is open")
        );
    }

    #[tokio::test]
    async fn test_circuit_breaker_recovery() {
        let circuit_breaker = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout: Duration::from_millis(50),
            success_threshold: 1,
            ..CircuitBreakerConfig::default()
        });

        let mut executor = RetryExecutor::with_circuit_breaker(
            RetryConfig {
                max_attempts: 1,
                ..RetryConfig::default()
            },
            circuit_breaker,
        );

        // Cause failure to open circuit
        let _ = executor
            .execute(|| async { Err::<i32, RustezeError>(RustezeError::network("Failure")) })
            .await;

        // Wait for recovery timeout
        sleep(Duration::from_millis(60)).await;

        // Should allow request and succeed, closing the circuit
        let result = executor
            .execute(|| async { Ok::<i32, RustezeError>(42) })
            .await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(
            executor.circuit_breaker_state(),
            Some(&CircuitBreakerState::Closed)
        );
    }
}
