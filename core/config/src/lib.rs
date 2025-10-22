use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RoutesConfig {
    pub project: Option<ProjectConfig>,
    pub deployment: Option<DeploymentConfig>,
    pub route: std::collections::HashMap<String, RouteConfig>,
    pub topic: Option<std::collections::HashMap<String, TopicConfig>>,
    pub db: Option<std::collections::HashMap<String, DbConfig>>,
    pub auth: Option<std::collections::HashMap<String, AuthHandlerConfig>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProjectConfig {
    pub name: String,
    pub version: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DeploymentConfig {
    pub runtime: Option<String>,
    pub memory: Option<String>,
    pub timeout: Option<String>,
    pub environment: Option<String>,
    pub arn: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RouteConfig {
    pub method: String,
    pub path: String,
    pub binary: String,
    pub description: Option<String>,
    pub parameters: Option<std::collections::HashMap<String, ParameterConfig>>,
    pub deployment: Option<DeploymentConfig>,
    pub auth: Option<AuthConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AuthConfig {
    pub handler: String,
    pub required: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ParameterConfig {
    #[serde(rename = "type")]
    pub param_type: String,
    pub source: String,
    pub required: Option<bool>,
    pub description: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TopicConfig {
    pub name: String,
    pub subscribers: std::collections::HashMap<String, SubscriberConfig>,
    pub description: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SubscriberConfig {
    pub binary: String,
    pub description: Option<String>,
    pub deployment: Option<DeploymentConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DbConfig {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AuthHandlerConfig {
    pub binary: String,
    pub description: Option<String>,
    pub deployment: Option<DeploymentConfig>,
}
