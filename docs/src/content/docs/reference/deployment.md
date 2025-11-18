---
title: Deployment
description: Deployment intro
---

## AWS Deployment

### Prerequisites

1. **AWS CLI configured** with appropriate credentials
2. **cargo-lambda** - Install with: `cargo install cargo-lambda`

### IAM Role Management

The deployment process automatically handles IAM role creation. When you deploy, Rusteze will:

- Check if the `lambda-execution-role` exists in your AWS account
- Create the role if it doesn't exist, with the appropriate trust policy for Lambda
- Attach the `AWSLambdaBasicExecutionRole` policy for basic Lambda execution permissions

No manual IAM setup is required!

### Deploying your service

1. **Generate the deployment artifacts**:

   ```bash
   cargo rusteze codegen
   ```

2. **Deploy to AWS**:
   ```bash
   cargo rusteze deploy
   ```

The deployment will:

- Build all Lambda functions from your routes
- Create/update Lambda functions in AWS
- Set up API Gateway with proper routing
- Configure permissions between API Gateway and Lambda

### Configuration

Update your `rusteze.toml` file:

```toml
service_name = "my-service"

[deployment]
provider = "aws"
region = "us-east-1"  # Change to your preferred region
type = "lambda"

[lambda]
memory_size = 256  # MB
```

### Example Output

After successful deployment, you'll see:

```
🚀 Deployment complete!
API Gateway ID: abc123def456
API URL: https://abc123def456.execute-api.us-east-1.amazonaws.com/prod

Available endpoints:
  GET /hello/{word} -> https://abc123def456.execute-api.us-east-1.amazonaws.com/prod/hello/{word}
  POST /calculate -> https://abc123def456.execute-api.us-east-1.amazonaws.com/prod/calculate
  GET /status -> https://abc123def456.execute-api.us-east-1.amazonaws.com/prod/status
```

### Testing your deployment

```bash
# Test a GET endpoint
curl https://your-api-id.execute-api.region.amazonaws.com/prod/hello/world

# Test a POST endpoint
curl -X POST https://your-api-id.execute-api.region.amazonaws.com/prod/calculate \
  -H "Content-Type: application/json" \
  -d '{"a": "5", "b": "3"}'
```
