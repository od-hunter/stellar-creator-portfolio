use actix_web::{middleware, web, App, HttpResponse, HttpServer};
use serde::{Deserialize, Serialize};
use tracing_subscriber;
use std::process;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub api_host: String,
    pub api_port: u16,
    pub database_url: String,
    pub redis_url: String,
    pub stellar_network: String,
    pub stellar_rpc_url: String,
    pub jwt_secret: String,
    pub jwt_expiry_seconds: u64,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, String> {
        let api_host = std::env::var("API_HOST")
            .unwrap_or_else(|_| "127.0.0.1".to_string());
        
        let api_port = std::env::var("API_PORT")
            .unwrap_or_else(|_| "3001".to_string())
            .parse::<u16>()
            .map_err(|_| "API_PORT must be a valid port number (1-65535)".to_string())?;
        
        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| "DATABASE_URL environment variable is required".to_string())?;
        
        // Validate database URL format
        if !database_url.starts_with("postgres://") && !database_url.starts_with("postgresql://") {
            return Err("DATABASE_URL must be a valid PostgreSQL connection string".to_string());
        }
        
        let redis_url = std::env::var("REDIS_URL")
            .unwrap_or_else(|_| "redis://localhost:6379".to_string());
        
        // Validate Redis URL format
        if !redis_url.starts_with("redis://") && !redis_url.starts_with("rediss://") {
            return Err("REDIS_URL must be a valid Redis connection string".to_string());
        }
        
        let stellar_network = std::env::var("STELLAR_NETWORK")
            .unwrap_or_else(|_| "testnet".to_string());
        
        // Validate Stellar network
        if !["testnet", "futurenet", "mainnet", "standalone"].contains(&stellar_network.as_str()) {
            return Err(format!(
                "STELLAR_NETWORK must be one of: testnet, futurenet, mainnet, standalone. Got: {}",
                stellar_network
            ));
        }
        
        let stellar_rpc_url = std::env::var("STELLAR_RPC_URL")
            .map_err(|_| "STELLAR_RPC_URL environment variable is required".to_string())?;
        
        // Validate RPC URL format
        if !stellar_rpc_url.starts_with("http://") && !stellar_rpc_url.starts_with("https://") {
            return Err("STELLAR_RPC_URL must be a valid HTTP/HTTPS URL".to_string());
        }
        
        let jwt_secret = std::env::var("JWT_SECRET")
            .map_err(|_| "JWT_SECRET environment variable is required".to_string())?;
        
        // Validate JWT secret strength
        if jwt_secret.len() < 32 {
            return Err("JWT_SECRET must be at least 32 characters for security".to_string());
        }
        
        if jwt_secret == "change_me_in_production" {
            tracing::warn!("⚠️  WARNING: Using default JWT_SECRET. Change this in production!");
        }
        
        let jwt_expiry_seconds = std::env::var("JWT_EXPIRY_SECONDS")
            .unwrap_or_else(|_| "3600".to_string())
            .parse::<u64>()
            .map_err(|_| "JWT_EXPIRY_SECONDS must be a valid number".to_string())?;
        
        if jwt_expiry_seconds < 300 {
            return Err("JWT_EXPIRY_SECONDS must be at least 300 (5 minutes)".to_string());
        }
        
        Ok(AppConfig {
            api_host,
            api_port,
            database_url,
            redis_url,
            stellar_network,
            stellar_rpc_url,
            jwt_secret,
            jwt_expiry_seconds,
        })
    }
    
    pub fn validate(&self) -> Result<(), String> {
        tracing::info!("Validating configuration...");
        tracing::info!("  API Host: {}", self.api_host);
        tracing::info!("  API Port: {}", self.api_port);
        tracing::info!("  Database URL: {}", mask_credentials(&self.database_url));
        tracing::info!("  Redis URL: {}", mask_credentials(&self.redis_url));
        tracing::info!("  Stellar Network: {}", self.stellar_network);
        tracing::info!("  Stellar RPC URL: {}", self.stellar_rpc_url);
        tracing::info!("  JWT Expiry: {} seconds", self.jwt_expiry_seconds);
        tracing::info!("✓ Configuration validated successfully");
        Ok(())
    }
}

fn mask_credentials(url: &str) -> String {
    if let Some(at_pos) = url.find('@') {
        if let Some(protocol_end) = url.find("://") {
            let protocol = &url[..protocol_end + 3];
            let after_at = &url[at_pos..];
            return format!("{}***:***{}", protocol, after_at);
        }
    }
    url.to_string()
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct BountyRequest {
    pub creator: String,
    pub title: String,
    pub description: String,
    pub budget: i128,
    pub deadline: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct BountyApplication {
    pub bounty_id: u64,
    pub freelancer: String,
    pub proposal: String,
    pub proposed_budget: i128,
    pub timeline: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct FreelancerRegistration {
    pub name: String,
    pub discipline: String,
    pub bio: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
    pub message: Option<String>,
}

impl<T> ApiResponse<T> {
    fn ok(data: T, message: Option<String>) -> Self {
        ApiResponse {
            success: true,
            data: Some(data),
            error: None,
            message,
        }
    }

    #[allow(dead_code)]
    fn err(error: String) -> Self
    where
        T: Default,
    {
        ApiResponse {
            success: false,
            data: None,
            error: Some(error),
            message: None,
        }
    }
}

async fn health() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({
        "status": "healthy",
        "service": "stellar-api",
        "version": "0.1.0"
    }))
}

async fn create_bounty(body: web::Json<BountyRequest>) -> HttpResponse {
    tracing::info!("Creating bounty: {:?}", body.title);
    let response: ApiResponse<serde_json::Value> = ApiResponse::ok(
        serde_json::json!({
            "bounty_id": 1,
            "creator": body.creator,
            "title": body.title,
            "budget": body.budget,
            "status": "open"
        }),
        Some("Bounty created successfully".to_string()),
    );
    HttpResponse::Created().json(response)
}

async fn list_bounties() -> HttpResponse {
    let response: ApiResponse<serde_json::Value> = ApiResponse::ok(
        serde_json::json!({ "bounties": [], "total": 0, "page": 1, "limit": 10 }),
        None,
    );
    HttpResponse::Ok().json(response)
}

async fn get_bounty(path: web::Path<u64>) -> HttpResponse {
    let bounty_id = path.into_inner();
    let response: ApiResponse<serde_json::Value> = ApiResponse::ok(
        serde_json::json!({ "id": bounty_id, "title": "Sample Bounty", "status": "open" }),
        None,
    );
    HttpResponse::Ok().json(response)
}

async fn apply_for_bounty(
    path: web::Path<u64>,
    body: web::Json<BountyApplication>,
) -> HttpResponse {
    let bounty_id = path.into_inner();
    let response: ApiResponse<serde_json::Value> = ApiResponse::ok(
        serde_json::json!({
            "application_id": 1,
            "bounty_id": bounty_id,
            "freelancer": body.freelancer,
            "status": "pending"
        }),
        Some("Application submitted successfully".to_string()),
    );
    HttpResponse::Created().json(response)
}

async fn register_freelancer(body: web::Json<FreelancerRegistration>) -> HttpResponse {
    let response: ApiResponse<serde_json::Value> = ApiResponse::ok(
        serde_json::json!({
            "name": body.name,
            "discipline": body.discipline,
            "verified": false
        }),
        Some("Freelancer registered successfully".to_string()),
    );
    HttpResponse::Created().json(response)
}

async fn list_freelancers(
    query: web::Query<std::collections::HashMap<String, String>>,
) -> HttpResponse {
    let discipline = query.get("discipline").cloned().unwrap_or_default();
    let response: ApiResponse<serde_json::Value> = ApiResponse::ok(
        serde_json::json!({
            "freelancers": [],
            "total": 0,
            "filters": { "discipline": discipline }
        }),
        None,
    );
    HttpResponse::Ok().json(response)
}

async fn get_freelancer(path: web::Path<String>) -> HttpResponse {
    let address = path.into_inner();
    let response: ApiResponse<serde_json::Value> = ApiResponse::ok(
        serde_json::json!({
            "address": address,
            "discipline": "UI/UX Design",
            "rating": 4.8,
            "completed_projects": 0
        }),
        None,
    );
    HttpResponse::Ok().json(response)
}

async fn get_escrow(path: web::Path<u64>) -> HttpResponse {
    let escrow_id = path.into_inner();
    let response: ApiResponse<serde_json::Value> = ApiResponse::ok(
        serde_json::json!({ "id": escrow_id, "status": "active", "amount": 0 }),
        None,
    );
    HttpResponse::Ok().json(response)
}

async fn release_escrow(path: web::Path<u64>) -> HttpResponse {
    let escrow_id = path.into_inner();
    let response: ApiResponse<serde_json::Value> = ApiResponse::ok(
        serde_json::json!({ "id": escrow_id, "status": "released" }),
        Some("Funds released successfully".to_string()),
    );
    HttpResponse::Ok().json(response)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,stellar_api=debug".to_string()),
        )
        .init();

    tracing::info!("🚀 Stellar API Service Starting...");
    
    // Load and validate configuration
    let config = match AppConfig::from_env() {
        Ok(cfg) => cfg,
        Err(e) => {
            tracing::error!("❌ Configuration error: {}", e);
            tracing::error!("Please check your environment variables and try again.");
            process::exit(1);
        }
    };
    
    // Validate configuration
    if let Err(e) = config.validate() {
        tracing::error!("❌ Configuration validation failed: {}", e);
        process::exit(1);
    }

    tracing::info!("Starting Stellar API on {}:{}", config.api_host, config.api_port);

    HttpServer::new(|| {
        App::new()
            .wrap(middleware::Logger::default())
            .wrap(middleware::NormalizePath::trim())
            .route("/health", web::get().to(health))
            .route("/api/bounties", web::post().to(create_bounty))
            .route("/api/bounties", web::get().to(list_bounties))
            .route("/api/bounties/{id}", web::get().to(get_bounty))
            .route("/api/bounties/{id}/apply", web::post().to(apply_for_bounty))
            .route("/api/freelancers/register", web::post().to(register_freelancer))
            .route("/api/freelancers", web::get().to(list_freelancers))
            .route("/api/freelancers/{address}", web::get().to(get_freelancer))
            .route("/api/escrow/{id}", web::get().to(get_escrow))
            .route("/api/escrow/{id}/release", web::post().to(release_escrow))
    })
    .bind((config.api_host.as_str(), config.api_port))?
    .run()
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_response_ok() {
        let response: ApiResponse<String> = ApiResponse::ok("test".to_string(), None);
        assert!(response.success);
        assert_eq!(response.data, Some("test".to_string()));
    }

    #[test]
    fn test_api_response_err() {
        let response: ApiResponse<String> = ApiResponse::err("error".to_string());
        assert!(!response.success);
        assert_eq!(response.error, Some("error".to_string()));
    }
    
    #[test]
    fn test_config_validation_missing_database_url() {
        std::env::remove_var("DATABASE_URL");
        std::env::remove_var("STELLAR_RPC_URL");
        std::env::remove_var("JWT_SECRET");
        
        let result = AppConfig::from_env();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("DATABASE_URL"));
    }
    
    #[test]
    fn test_config_validation_invalid_port() {
        std::env::set_var("API_PORT", "99999");
        std::env::set_var("DATABASE_URL", "postgres://user:pass@localhost/db");
        std::env::set_var("STELLAR_RPC_URL", "https://soroban-testnet.stellar.org");
        std::env::set_var("JWT_SECRET", "test_secret_key_with_at_least_32_chars");
        
        let result = AppConfig::from_env();
        assert!(result.is_err());
    }
    
    #[test]
    fn test_config_validation_invalid_stellar_network() {
        std::env::set_var("API_PORT", "3001");
        std::env::set_var("DATABASE_URL", "postgres://user:pass@localhost/db");
        std::env::set_var("STELLAR_NETWORK", "invalid_network");
        std::env::set_var("STELLAR_RPC_URL", "https://soroban-testnet.stellar.org");
        std::env::set_var("JWT_SECRET", "test_secret_key_with_at_least_32_chars");
        
        let result = AppConfig::from_env();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("STELLAR_NETWORK"));
    }
    
    #[test]
    fn test_config_validation_weak_jwt_secret() {
        std::env::set_var("API_PORT", "3001");
        std::env::set_var("DATABASE_URL", "postgres://user:pass@localhost/db");
        std::env::set_var("STELLAR_RPC_URL", "https://soroban-testnet.stellar.org");
        std::env::set_var("JWT_SECRET", "short");
        
        let result = AppConfig::from_env();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("at least 32 characters"));
    }
    
    #[test]
    fn test_config_validation_success() {
        std::env::set_var("API_PORT", "3001");
        std::env::set_var("DATABASE_URL", "postgres://user:pass@localhost/db");
        std::env::set_var("STELLAR_NETWORK", "testnet");
        std::env::set_var("STELLAR_RPC_URL", "https://soroban-testnet.stellar.org");
        std::env::set_var("JWT_SECRET", "test_secret_key_with_at_least_32_characters_for_security");
        
        let result = AppConfig::from_env();
        assert!(result.is_ok());
        
        let config = result.unwrap();
        assert_eq!(config.api_port, 3001);
        assert_eq!(config.stellar_network, "testnet");
    }
}
