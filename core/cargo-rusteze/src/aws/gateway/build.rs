use core::fmt;
use std::fmt::Display;

use crate::RustezeConfig;
use aws_sdk_apigatewayv2::Client as ApiGatewayClient;

#[derive(Debug)]
pub enum ApiGatewayDeploymentError {
    GatewayUpdateError(String, String),
    GatewayCreationError(String),
    GatewayFetchError(String),
}

impl Display for ApiGatewayDeploymentError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ApiGatewayDeploymentError::GatewayUpdateError(id, message) => {
                write!(f, "Unable to update apiGateway {}: {}", id, message)
            }
            ApiGatewayDeploymentError::GatewayCreationError(message) => {
                write!(f, "Unable to create ApiGateway {}", message)
            }
            ApiGatewayDeploymentError::GatewayFetchError(message) => {
                write!(f, "Unable to fetch list of gateways: {}", message)
            }
        }
    }
}

impl std::error::Error for ApiGatewayDeploymentError {}

pub async fn build_api_gateway(
    config: &RustezeConfig,
    api_client: &ApiGatewayClient,
) -> Result<String, ApiGatewayDeploymentError> {
    let api_name = &config.service_name;

    let api_fetch_result = api_client.get_apis().send().await;

    let apis = match api_fetch_result {
        Ok(apis) => apis,
        Err(e) => return Err(ApiGatewayDeploymentError::GatewayFetchError(e.to_string())),
    };

    let existing_api = apis.items().iter().find(|a| a.name() == Some(api_name));

    let api_id = match existing_api {
        Some(api) => {
            println!(
                "Using existing API Gateway: {} - {:?}",
                api_name,
                &api.api_endpoint().unwrap()
            );

            let id = match api.api_id() {
                Some(id) => id.to_string(),
                None => panic!("Unable to find existing API Gateway"),
            };

            id
        }
        None => {
            println!("Creating new API Gateway: {}", api_name);
            let api = match api_client
                .create_api()
                .name(api_name)
                .protocol_type(aws_sdk_apigatewayv2::types::ProtocolType::Http)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    return Err(ApiGatewayDeploymentError::GatewayCreationError(
                        e.to_string(),
                    ));
                }
            };
            api.api_id().unwrap().to_string()
        }
    };

    Ok(api_id)
}

pub async fn create_routes() {}

// #[cfg(test)]
// mod tests {

//     use super::*;

//     #[test]
//     fn test_delete_routes() {
//         let s = "name = 'users'".to_string();
//         assert_eq!(get_db_attributes(s), "users")
//     }
// }
