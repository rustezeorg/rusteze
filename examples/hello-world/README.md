# Hello World Example

This is a simple hello world example for Rusteze that demonstrates API routes, authentication, and pub/sub functionality.

## Running locally

```bash
cargo rusteze serve
```

## Deploying to AWS

### Prerequisites

1. **AWS CLI configured** with appropriate credentials
2. **IAM Role for Lambda execution** - see [deployment guide](../../docs/deployment.md)

### Deploy

```bash
# Generate deployment artifacts (if not already done)
cargo rusteze codegen

# Deploy to AWS
cargo rusteze deploy
```

This will:

- Build all Lambda functions from your routes
- Create/update Lambda functions in AWS
- Set up API Gateway with proper routing
- Configure permissions between API Gateway and Lambda

### Configuration

The deployment is configured in `rusteze.toml`:

```toml
service_name = "hello-world"

[deployment]
provider = "aws"
region = "ap-southeast-2"  # Change to your preferred region
type = "lambda"

[lambda]
memory_size = 256  # MB
```

## Endpoints

- `GET /hello/{word}` - Returns a greeting
- `POST /calculate` - Adds two numbers (as strings)
- `GET /status` - Returns service status
- `PUT /toggle` - Toggles a feature
- `POST /test` - Test endpoint with pub/sub
- `GET /protected` - Protected endpoint (requires auth)
- `POST /admin/users` - Admin endpoint (requires auth)

## Testing deployed endpoints

After deployment, you'll get an API URL. Test with:

```bash
# Test a GET endpoint
curl https://your-api-id.execute-api.ap-southeast-2.amazonaws.com/prod/hello/world

# Test a POST endpoint
curl -X POST https://your-api-id.execute-api.ap-southeast-2.amazonaws.com/prod/calculate \
  -H "Content-Type: application/json" \
  -d '{"a": "5", "b": "3"}'

# Test status endpoint
curl https://your-api-id.execute-api.ap-southeast-2.amazonaws.com/prod/status
```
