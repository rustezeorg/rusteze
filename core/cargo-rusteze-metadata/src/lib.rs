use std::fs;

use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct RustezeConfig {
    pub service_name: String,
    pub deployment: Deployment
}

#[derive(Deserialize, Debug, Clone)]
pub struct Deployment {
    pub provider: String,
    pub region: String,

    #[serde(rename(deserialize = "type"))]
    pub deployment_type: Option<String>,
    pub lambda: Option<LambdaDeploymentOptions>,
}

#[derive(Deserialize, Debug, Clone, Copy)]
pub struct LambdaDeploymentOptions {
    pub memory_size: i64
}


fn convert_config(config_content: String) -> RustezeConfig {
    let config = match toml::from_str(&config_content) {
        Ok(s) => s,
        Err(err) => panic!("Unable to parse rusteze.toml file: {:?}", err),
    };
    config
}

pub fn read_config() -> RustezeConfig {
    let config_content = fs::read_to_string("rusteze.toml").expect("Failed to read rusteze.toml");
    convert_config(config_content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_read_basic_config_file() {
        let config_string = r#"
            service_name = "hello-world"

            [deployment]
            provider = "aws"
            region = "ap-southeast-2"
            "#;

        let config = convert_config(config_string.to_string());

        assert_eq!(config.service_name, "hello-world");
        assert_eq!(config.deployment.provider, "aws");
    }

    #[test]
    fn can_read_deploy_config_file() {
        let config_string = r#"
            service_name = "hello-world"

            [deployment]
            provider = "aws"
            region = "ap-southeast-2"

            type = "lambda"

            [lambda]
            memory_size = 2048
            "#;

        let config = convert_config(config_string.to_string());

        assert_eq!(config.service_name, "hello-world");
        assert_eq!(config.deployment.provider, "aws");
        assert_eq!(config.deployment.deployment_type.unwrap(), "lambda");
    }
}