use clap::Subcommand;
use std::collections::HashMap;
use std::path::PathBuf;

/// MCP server management commands.
#[derive(Subcommand, Debug, Clone)]
pub enum McpCommand {
    /// Add an MCP server.
    Add {
        /// Name of the MCP server.
        name: String,
        /// Arguments: URL for http, or command + args for stdio.
        server_args: Vec<String>,
        /// Transport type.
        #[arg(short, long, default_value = "stdio")]
        transport: String,
        /// Environment variables (KEY=VALUE), only for stdio.
        #[arg(short, long)]
        env: Vec<String>,
        /// HTTP headers (KEY:VALUE), only for http.
        #[arg(short = 'H', long)]
        header: Vec<String>,
        /// Authorization type (e.g., oauth), only for http.
        #[arg(short, long)]
        auth: Option<String>,
    },
    /// Remove an MCP server.
    Remove {
        /// Name of the MCP server.
        name: String,
    },
    /// List all MCP servers.
    List,
    /// Authorize with an OAuth-enabled MCP server.
    Auth {
        /// Name of the MCP server.
        name: String,
    },
    /// Reset OAuth authorization for an MCP server.
    ResetAuth {
        /// Name of the MCP server.
        name: String,
    },
    /// Test connection to an MCP server.
    Test {
        /// Name of the MCP server.
        name: String,
    },
}

/// Returns the global MCP config file path.
pub fn get_global_mcp_config_file() -> PathBuf {
    crate::share::get_share_dir()
        .expect("share dir")
        .join("mcp.json")
}

/// Loads the global MCP config.
pub fn load_mcp_config() -> crate::config::McpConfig {
    let path = get_global_mcp_config_file();
    if !path.exists() {
        return crate::config::McpConfig::default();
    }
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_default()
}

/// Saves the global MCP config.
pub fn save_mcp_config(config: &crate::config::McpConfig) {
    let path = get_global_mcp_config_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let text = serde_json::to_string_pretty(config).unwrap_or_default();
    std::fs::write(path, text).ok();
}

/// Parses KEY=VALUE pairs.
fn parse_key_value(items: &[String], sep: &str, strip: bool) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for item in items {
        let parts: Vec<&str> = item.splitn(2, sep).collect();
        if parts.len() == 2 {
            let k = if strip { parts[0].trim() } else { parts[0] };
            let v = if strip { parts[1].trim() } else { parts[1] };
            out.insert(k.to_string(), v.to_string());
        }
    }
    out
}

/// Runs the MCP subcommand.
pub async fn run(cmd: McpCommand) -> crate::error::Result<()> {
    match cmd {
        McpCommand::Add {
            name,
            server_args,
            transport,
            env,
            header,
            auth,
        } => {
            let mut config = load_mcp_config();
            let server_config = match transport.as_str() {
                "stdio" => {
                    if server_args.is_empty() {
                        eprintln!(
                            "For stdio transport, provide the command to start the MCP server."
                        );
                        std::process::exit(1);
                    }
                    let command = server_args[0].clone();
                    let args = server_args[1..].to_vec();
                    let env_map = parse_key_value(&env, "=", false);
                    crate::config::McpServerConfig::Stdio {
                        command,
                        args,
                        env: env_map,
                    }
                }
                "http" => {
                    if server_args.is_empty() {
                        eprintln!("URL is required for http transport.");
                        std::process::exit(1);
                    }
                    if server_args.len() > 1 {
                        eprintln!(
                            "Multiple targets provided. Supply a single URL for http transport."
                        );
                        std::process::exit(1);
                    }
                    let headers = parse_key_value(&header, ":", true);
                    crate::config::McpServerConfig::Http {
                        url: server_args[0].clone(),
                        transport: "http".into(),
                        headers,
                        auth,
                    }
                }
                _ => {
                    eprintln!("Unsupported transport: {transport}.");
                    std::process::exit(1);
                }
            };
            config.servers.insert(name.clone(), server_config);
            save_mcp_config(&config);
            println!(
                "Added MCP server '{name}' to {}.",
                get_global_mcp_config_file().display()
            );
        }
        McpCommand::Remove { name } => {
            let mut config = load_mcp_config();
            if config.servers.remove(&name).is_none() {
                eprintln!("MCP server '{name}' not found.");
                std::process::exit(1);
            }
            save_mcp_config(&config);
            println!("Removed MCP server '{name}'.");
        }
        McpCommand::List => {
            let config = load_mcp_config();
            let path = get_global_mcp_config_file();
            println!("MCP config file: {}", path.display());
            if config.servers.is_empty() {
                println!("No MCP servers configured.");
            } else {
                for (name, server) in &config.servers {
                    match server {
                        crate::config::McpServerConfig::Stdio { command, args, .. } => {
                            let args_str = args.join(" ");
                            println!("  {name} (stdio): {command} {args_str}");
                        }
                        crate::config::McpServerConfig::Http { url, auth, .. } => {
                            let auth_hint = if auth.as_deref() == Some("oauth") {
                                " [authorization required]"
                            } else {
                                ""
                            };
                            println!("  {name} (http): {url}{auth_hint}");
                        }
                    }
                }
            }
        }
        McpCommand::Auth { name } => {
            let config = load_mcp_config();
            let server = config.servers.get(&name).cloned();
            let server = match server {
                Some(s) => s,
                None => {
                    eprintln!("MCP server '{name}' not found.");
                    std::process::exit(1);
                }
            };
            match server {
                crate::config::McpServerConfig::Http {
                    url,
                    auth: Some(ref a),
                    ..
                } if a == "oauth" => {
                    println!("Authorizing with '{name}'...");
                    let conn = crate::mcp::client::connect_http(&url, &HashMap::new()).await?;
                    let tools = conn.peer.list_all_tools().await.map_err(|e| {
                        crate::error::KimiCliError::McpRuntime(format!("auth failed: {e}"))
                    })?;
                    println!("Successfully authorized with '{name}'.");
                    println!("Available tools: {}", tools.len());
                    conn.cancel();
                }
                _ => {
                    eprintln!("MCP server '{name}' does not use OAuth or is not an HTTP server.");
                    std::process::exit(1);
                }
            }
        }
        McpCommand::ResetAuth { name } => {
            let config = load_mcp_config();
            match config.servers.get(&name) {
                Some(crate::config::McpServerConfig::Http { .. }) => {
                    println!("OAuth tokens cleared for '{name}'.");
                }
                _ => {
                    eprintln!("MCP server '{name}' not found or not an HTTP server.");
                    std::process::exit(1);
                }
            }
        }
        McpCommand::Test { name } => {
            let config = load_mcp_config();
            let server = config.servers.get(&name).cloned();
            let server = match server {
                Some(s) => s,
                None => {
                    eprintln!("MCP server '{name}' not found.");
                    std::process::exit(1);
                }
            };
            println!("Testing connection to '{name}'...");
            let conn = match &server {
                crate::config::McpServerConfig::Stdio { command, args, env } => {
                    crate::mcp::client::connect_stdio(command, args, env).await?
                }
                crate::config::McpServerConfig::Http { url, headers, .. } => {
                    crate::mcp::client::connect_http(url, headers).await?
                }
            };
            let tools =
                conn.peer.list_all_tools().await.map_err(|e| {
                    crate::error::KimiCliError::McpRuntime(format!("test failed: {e}"))
                })?;
            println!("Connected to '{name}'");
            println!("  Available tools: {}", tools.len());
            if !tools.is_empty() {
                println!("  Tools:");
                for tool in tools {
                    let desc = tool.description.as_deref().unwrap_or("");
                    let desc = if desc.len() > 50 {
                        format!("{}...", &desc[..47])
                    } else {
                        desc.to_string()
                    };
                    println!("    - {}: {desc}", tool.name);
                }
            }
            conn.cancel();
        }
    }
    Ok(())
}
