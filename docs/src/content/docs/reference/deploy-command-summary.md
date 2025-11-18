---
title: Deployment command
description: some description
---

# Deploy Command Implementation Summary

## Overview

The `cargo rusteze deploy` command provides automated deployment of Rusteze applications to AWS using Lambda and API Gateway.

## What it does

### 1. Configuration Reading

- Reads `rusteze.toml` for deployment configuration (provider, region, memory settings)
- Reads `.rusteze/manifest.json` for generated function metadata

### 2. Function Building

- Builds all Lambda functions in the `.rusteze` directory using `cargo lambda build --release --output-format zip`
- Uses cargo-lambda for proper cross-compilation and Lambda packaging
- Creates optimized zip deployment packages for each function

### 3. AWS Deployment

- **Route Functions**: Deploys HTTP endpoint handlers as Lambda functions
- **Auth Functions**: Deploys authentication handlers
- **Subscriber Functions**: Deploys pub/sub topic subscribers
- **API Gateway**: Creates HTTP API with proper routing and integrations

### 4. Manifest Updates

- Updates `manifest.json` with deployed ARNs for tracking
- Stores API Gateway ID for future deployments

## Usage

```bash
# Generate deployment artifacts
cargo rusteze codegen

# Deploy to AWS
cargo rusteze deploy
```

## Prerequisites

1. AWS CLI configured with appropriate credentials
2. **cargo-lambda** installed: `cargo install cargo-lambda`
3. Generated `.rusteze` directory from codegen

Note: IAM roles are automatically created during deployment if they don't exist.

## Configuration

In `rusteze.toml`:

```toml
service_name = "my-service"

[deployment]
provider = "aws"
region = "us-east-1"
type = "lambda"

[lambda]
memory_size = 256
```

## Output

The command provides:

- Deployment progress with function-by-function status
- API Gateway URL for testing
- List of available endpoints
- Updated manifest with ARNs

## Example Output

```
🚀 Deployment complete!
API Gateway ID: abc123def456
API URL: https://abc123def456.execute-api.us-east-1.amazonaws.com/prod

Available endpoints:
  GET /hello/{word} -> https://abc123def456.execute-api.us-east-1.amazonaws.com/prod/hello/{word}
  POST /calculate -> https://abc123def456.execute-api.us-east-1.amazonaws.com/prod/calculate
  GET /status -> https://abc123def456.execute-api.us-east-1.amazonaws.com/prod/status
```

## Architecture

- Each route becomes a separate Lambda function
- API Gateway HTTP API handles routing and integration
- Lambda functions use custom runtime (`provided.al2`)
- Functions are named with pattern: `{service_name}-{binary_name}`
- ARNs are tracked in manifest for updates and management

## Error Handling

- Handles existing functions by updating code
- Provides clear error messages for missing prerequisites
- Validates AWS credentials and permissions
- Graceful handling of API Gateway conflicts
