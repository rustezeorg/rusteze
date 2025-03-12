use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};


#[derive(Serialize, Deserialize)]
struct RouteConfig {
    functions: Vec<FunctionConfig>,
}

#[derive(Serialize, Deserialize)]
struct FunctionConfig {
    name: String,
    path: String,
    method: String,
}

#[proc_macro_attribute]
pub fn route(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    let func_name = &input.sig.ident;

    let attr_args = attr.to_string();
    let mut method = "GET".to_string();
    let mut path = "/".to_string();

    let args: Vec<&str> = attr_args.split(',').map(|s| s.trim()).collect();

    for arg in args {
        if arg.starts_with("method =") {
            method = arg.trim_start_matches("method =").trim_matches('"').to_string();
        } else if arg.starts_with("path =") {
            path = arg.trim_start_matches("path =").trim_matches('"').to_string();
        }
    }

    // @todo - Should move this to the lib to be shared between the codegen and the deploy command.
    let deploy_file = "rusteze.deploy.toml";
    let mut config: RouteConfig = if let Ok(content) = std::fs::read_to_string(deploy_file) {
        toml::from_str(&content).unwrap_or(RouteConfig { functions: vec![] })
    } else {
        RouteConfig { functions: vec![] }
    };

    config.functions.push(FunctionConfig {
        name: func_name.to_string(),
        path,
        method,
    });

    let toml_string = toml::to_string(&config).expect("Failed to serialize TOML");
    let mut file = BufWriter::new(File::create(deploy_file).expect("Failed to open deploy.toml"));
    file.write_all(toml_string.as_bytes()).expect("Failed to write to deploy.toml");

    let lambda_name = format!("lambda_{}", func_name);


    // This needs to be determined by a config file?
    let expanded = quote! {
        use rusteze::tokio;

        #input

        #[tokio::main]
        async fn main() -> Result<(), rusteze::lambda_runtime::Error> {
            use rusteze::lambda_runtime::{service_fn, LambdaEvent};
            use rusteze::aws_lambda_events::event::apigw::{ApiGatewayProxyRequest, ApiGatewayProxyResponse};
            use rusteze::aws_lambda_events::http::HeaderMap;
            use rusteze::serde_json::json;

            async fn handler(event: LambdaEvent<ApiGatewayProxyRequest>) -> Result<ApiGatewayProxyResponse, rusteze::lambda_runtime::Error> {
                let request = event.payload;
                let response_body = #func_name(request.body.unwrap_or_default()).into();

                let mut headers = HeaderMap::new();
                headers.insert("content-type", "application/json".parse().unwrap());

                let response = ApiGatewayProxyResponse {
                    status_code: 200,
                    body: Some(response_body),
                    headers,
                    ..Default::default()
                };

                Ok(response)
            }

            let func = service_fn(handler);
            rusteze::lambda_runtime::run(func).await?;
            Ok(())
        }
    };

    TokenStream::from(expanded)
}
