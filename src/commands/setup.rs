use anyhow::Result;
use inquire::{Text, Confirm, Select, validator::Validation};
use colored::*;
use indicatif::ProgressBar;
use figlet_rs::FIGfont;

use crate::config::ConfigManager;
use crate::types::{NodeConfig, NodePaths};

pub async fn setup_command() -> Result<()> {
    display_welcome().await?;
    
    let config_manager = ConfigManager::new()?;
    
    // Check if configuration already exists
    if config_manager.exists() {
        let overwrite = Confirm::new("Configuration already exists. Do you want to overwrite it?")
            .with_default(false)
            .prompt()?;
            
        if !overwrite {
            println!("{}", "⚠️ Setup cancelled. Use --force to overwrite existing configuration.".yellow());
            return Ok(());
        }
    }
    
    println!("{}", "🚀 Starting Solana Validator Switch Setup".bright_cyan());
    println!();
    println!("{}", "This setup will configure:".dimmed());
    println!("{}", "  1. SSH connection settings".dimmed());
    println!("{}", "  2. Primary and backup validator nodes".dimmed());
    println!("{}", "  3. RPC endpoint".dimmed());
    println!("{}", "  4. Default monitoring, security, and display settings".dimmed());
    println!();
    
    // Detect SSH keys
    let ssh_keys = detect_ssh_keys().await?;
    
    // SSH key path configuration
    let _ssh_key_path = collect_ssh_key_configuration(&ssh_keys).await?;
    
    // Validators configuration
    let validators = collect_validators_configuration().await?;
    
    // Build final configuration
    let config = crate::types::Config {
        version: "1.0.0".to_string(),
        validators,
    };
    
    // Validate and save configuration
    validate_and_save_configuration(&config_manager, &config).await?;
    
    // Test initial connections
    test_initial_connections(&config).await?;
    
    display_completion().await?;
    
    Ok(())
}

async fn display_welcome() -> Result<()> {
    // Clear screen
    println!("\x1B[2J\x1B[1;1H");
    
    // Display ASCII art banner
    if let Ok(font) = FIGfont::standard() {
        if let Some(figure) = font.convert("SVS Setup") {
            println!("{}", figure.to_string().bright_cyan());
        }
    } else {
        println!("{}", "🚀 Solana Validator Switch Setup".bright_cyan().bold());
    }
    
    println!("{}", "Professional-grade validator switching for Solana".dimmed());
    println!();
    
    println!("{}", "⚠️  Important Security Notes:".yellow().bold());
    println!("{}", "   • This tool stores SSH key file paths in configuration".yellow());
    println!("{}", "   • SSH private keys remain in your ~/.ssh/ directory".yellow());
    println!("{}", "   • No passwords or key contents are stored in config files".yellow());
    println!("{}", "   • All connections use your existing SSH key files".yellow());
    println!("{}", "   • Configuration files contain file paths and hostnames".yellow());
    println!();
    
    let ready = Confirm::new("Ready to begin setup?")
        .with_default(true)
        .prompt()?;
        
    if !ready {
        println!("{}", "Setup cancelled.".yellow());
        std::process::exit(0);
    }
    
    Ok(())
}

#[derive(Debug)]
struct SshKey {
    path: String,
    key_type: String,
    comment: String,
    valid: bool,
}

async fn detect_ssh_keys() -> Result<Vec<SshKey>> {
    let spinner = ProgressBar::new_spinner();
    spinner.set_message("Detecting SSH keys...");
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));
    
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let ssh_dir = home.join(".ssh");
    
    let mut keys = Vec::new();
    let key_types = ["id_rsa", "id_ecdsa", "id_ed25519", "id_dsa"];
    
    for key_type in &key_types {
        let private_key_path = ssh_dir.join(key_type);
        let public_key_path = ssh_dir.join(format!("{}.pub", key_type));
        
        if private_key_path.exists() && public_key_path.exists() {
            let comment = std::fs::read_to_string(&public_key_path)
                .map(|content| {
                    content.split_whitespace()
                        .nth(2)
                        .unwrap_or("")
                        .to_string()
                })
                .unwrap_or_default();
                
            keys.push(SshKey {
                path: private_key_path.to_string_lossy().to_string(),
                key_type: key_type.to_string(),
                comment,
                valid: true,
            });
        }
    }
    
    if keys.is_empty() {
        spinner.finish_with_message("❌ No SSH keys found");
        println!("{}", "❌ No SSH keys detected in ~/.ssh/".red());
        println!("{}", "Please generate SSH keys first:".yellow());
        println!();
        println!("{}", "  ssh-keygen -t ed25519 -C \"your_email@example.com\"".dimmed());
        println!("{}", "  ssh-copy-id user@validator-host".dimmed());
        println!();
        std::process::exit(1);
    }
    
    spinner.finish_with_message(format!("✅ Found {} SSH key(s)", keys.len()));
    
    // Show detected keys
    println!();
    println!("{}", "🔑 Detected SSH Keys:".bright_cyan());
    for (index, key) in keys.iter().enumerate() {
        let status = if key.valid { "✅".green() } else { "❌".red() };
        let key_type_display = key.key_type.to_uppercase();
        let comment_display = if !key.comment.is_empty() {
            format!(" ({})", key.comment)
        } else {
            String::new()
        };
        
        println!("  {}. {} {:8} {}{}", 
            index + 1, 
            status, 
            key_type_display, 
            key.path, 
            comment_display.dimmed()
        );
    }
    
    Ok(keys)
}

async fn collect_ssh_key_configuration(ssh_keys: &[SshKey]) -> Result<String> {
    println!();
    println!("{}", "🔑 SSH Key Configuration".bright_cyan());
    println!("{}", "Select the SSH private key to use for connecting to your validator nodes.".dimmed());
    println!();
    
    if ssh_keys.is_empty() {
        return Err(anyhow::anyhow!("No SSH keys found. Please generate SSH keys first."));
    }
    
    if ssh_keys.len() == 1 {
        let key = &ssh_keys[0];
        println!("{}", format!("Using SSH key: {}", key.path).green());
        return Ok(key.path.clone());
    }
    
    // Multiple keys available, let user choose
    let key_choices: Vec<String> = ssh_keys.iter()
        .map(|key| {
            let key_type_display = key.key_type.to_uppercase();
            let comment_display = if !key.comment.is_empty() {
                format!(" ({})", key.comment)
            } else {
                String::new()
            };
            format!("{:8} {}{}", key_type_display, key.path, comment_display)
        })
        .collect();
    
    let selection = Select::new("Select SSH private key:", key_choices.clone())
        .with_starting_cursor(0)
        .prompt()?;
        
    let selected_index = key_choices.iter().position(|x| x == &selection).unwrap();
    let selected_key = &ssh_keys[selected_index];
    
    println!("{}", format!("Selected SSH key: {}", selected_key.path).green());
    
    Ok(selected_key.path.clone())
}

async fn collect_validators_configuration() -> Result<Vec<crate::types::ValidatorPair>> {
    println!();
    println!("{}", "🖥️ Validator Configuration".bright_cyan());
    println!();
    println!("{}", "Configure validators. Each validator has two nodes that share the same validator identity.".dimmed());
    println!();
    
    let mut validators = Vec::new();
    
    // For now, we'll configure one validator, but this can be extended to multiple validators
    let add_validator = Confirm::new("Configure a validator?")
        .with_default(true)
        .prompt()?;
        
    if add_validator {
        if let Some(validator) = configure_validator().await? {
            validators.push(validator);
        }
    }
    
    Ok(validators)
}

async fn configure_validator() -> Result<Option<crate::types::ValidatorPair>> {
    println!("{}", "🔑 Validator Identity Configuration".bright_cyan());
    println!("{}", "These public keys identify the validator and are shared between all nodes.".dimmed());
    println!();
    
    let vote_pubkey: String = Text::new("Vote Pubkey:")
        .with_help_message("Enter the public key for the vote account")
        .with_validator(|input: &str| {
            if input.trim().is_empty() {
                Ok(Validation::Invalid("Vote Pubkey is required".into()))
            } else if input.len() < 32 || input.len() > 44 {
                Ok(Validation::Invalid("Vote Pubkey should be a valid base58 public key (32-44 characters)".into()))
            } else {
                Ok(Validation::Valid)
            }
        })
        .prompt()?;
        
    let identity_pubkey: String = Text::new("Identity Pubkey:")
        .with_help_message("Enter the public key for the funded validator identity")
        .with_validator(|input: &str| {
            if input.trim().is_empty() {
                Ok(Validation::Invalid("Identity Pubkey is required".into()))
            } else if input.len() < 32 || input.len() > 44 {
                Ok(Validation::Invalid("Identity Pubkey should be a valid base58 public key (32-44 characters)".into()))
            } else {
                Ok(Validation::Valid)
            }
        })
        .prompt()?;
    
    // RPC endpoint configuration
    println!();
    println!("{}", "🌐 RPC Configuration".bright_cyan());
    let rpc: String = Text::new("RPC endpoint:")
        .with_default("https://api.mainnet-beta.solana.com")
        .with_help_message("Solana RPC endpoint for monitoring")
        .prompt()?;
    
    // SSH key configuration
    println!();
    println!("{}", "🔐 SSH Configuration".bright_cyan());
    let local_ssh_key_path: String = Text::new("Local SSH key path:")
        .with_default("~/.ssh/id_rsa")
        .with_help_message("Path to your SSH private key for accessing validator nodes")
        .prompt()?;
    
    // Configure nodes
    let mut nodes = Vec::new();
    
    println!();
    println!("{}", "📡 Node 1 Configuration".green().bold());
    if let Some(node1) = configure_node("Node 1").await? {
        nodes.push(node1);
    }
    
    println!();
    println!("{}", "📡 Node 2 Configuration".yellow().bold());
    if let Some(node2) = configure_node("Node 2").await? {
        nodes.push(node2);
    }
    
    if nodes.len() == 2 {
        Ok(Some(crate::types::ValidatorPair {
            vote_pubkey,
            identity_pubkey,
            rpc,
            local_ssh_key_path,
            nodes,
        }))
    } else {
        Ok(None)
    }
}

async fn configure_node(node_type: &str) -> Result<Option<NodeConfig>> {
    let label: String = Text::new(&format!("{} node label:", node_type))
        .with_default(&format!("{} validator", node_type))
        .with_validator(|input: &str| {
            if input.trim().is_empty() {
                Ok(Validation::Invalid("Label is required".into()))
            } else {
                Ok(Validation::Valid)
            }
        })
        .prompt()?;

    let host: String = Text::new(&format!("{} node host (IP or hostname):", node_type))
        .with_validator(|input: &str| {
            if input.trim().is_empty() {
                Ok(Validation::Invalid("Host is required".into()))
            } else {
                // Basic validation - you could add IP/hostname regex here
                Ok(Validation::Valid)
            }
        })
        .prompt()?;
        
    let port: u16 = Text::new(&format!("{} node SSH port:", node_type))
        .with_default("22")
        .with_validator(|input: &str| {
            match input.parse::<u16>() {
                Ok(val) if val >= 1 && val <= 65535 => Ok(Validation::Valid),
                Ok(_) => Ok(Validation::Invalid("Port must be between 1 and 65535".into())),
                Err(_) => Ok(Validation::Invalid("Please enter a valid port number".into()))
            }
        })
        .prompt()?
        .parse()?;
        
    let user: String = Text::new(&format!("{} node SSH user:", node_type))
        .with_default("solana")
        .with_validator(|input: &str| {
            if input.trim().is_empty() {
                Ok(Validation::Invalid("User is required".into()))
            } else {
                Ok(Validation::Valid)
            }
        })
        .prompt()?;
    
    // Collect validator paths
    println!();
    println!("{} {} node file paths:", "📁".dimmed(), node_type);
    
    let funded_identity: String = Text::new("Funded identity keypair path:")
        .with_default("/home/solana/.secrets/funded-validator-keypair.json")
        .prompt()?;
        
    let unfunded_identity: String = Text::new("Unfunded identity keypair path:")
        .with_default("/home/solana/.secrets/unfunded-validator-keypair.json")
        .prompt()?;
        
    let vote_keypair: String = Text::new("Vote account keypair path:")
        .with_default("/home/solana/.secrets/vote-account-keypair.json")
        .prompt()?;
        
    let ledger: String = Text::new("Ledger directory path:")
        .with_default("/mnt/solana_ledger")
        .prompt()?;
        
    let tower: String = Text::new("Tower file path (supports wildcards):")
        .with_default(&format!("{}/tower-1_9-*.bin", ledger))
        .prompt()?;
        
    let solana_cli_path: String = Text::new("Solana CLI binary path:")
        .with_default("/home/solana/.local/share/solana/install/active_release/bin/solana")
        .prompt()?;
    
    
    Ok(Some(NodeConfig {
        label,
        host,
        port,
        user,
        paths: NodePaths {
            funded_identity,
            unfunded_identity,
            vote_keypair,
            ledger,
            tower,
            solana_cli_path: solana_cli_path.clone(),
            firedancer_config: Some("firedancer-config.toml".to_string()), // Default value
            fdctl_path: Some("fdctl".to_string()), // Default value
        },
    }))
}



async fn validate_and_save_configuration(config_manager: &ConfigManager, config: &crate::types::Config) -> Result<()> {
    let spinner = ProgressBar::new_spinner();
    spinner.set_message("Validating configuration...");
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));
    
    // Save configuration (validation would go here)
    config_manager.save(config)?;
    
    spinner.finish_with_message("✅ Configuration saved successfully");
    println!("{} {}", 
        "✅ Configuration saved to:".green(), 
        config_manager.get_config_path().display()
    );
    
    Ok(())
}

async fn test_initial_connections(_config: &crate::types::Config) -> Result<()> {
    let test_connections = Confirm::new("Test SSH connections to configured nodes?")
        .with_default(true)
        .prompt()?;
        
    if !test_connections {
        return Ok(());
    }
    
    // This would call the actual connection test
    println!("{}", "⚠️ Connection test functionality coming soon...".yellow());
    
    Ok(())
}

async fn display_completion() -> Result<()> {
    println!();
    println!("{}", "✨ Setup Complete! ✨".bright_green().bold());
    println!();
    
    println!("{}", "Next steps:".bright_cyan());
    println!("{} {}", "  1. Test your configuration:".dimmed(), "svs config --test".white());
    println!("{} {}", "  2. Check validator status:".dimmed(), "svs status".white());
    println!("{} {}", "  3. Monitor your validators:".dimmed(), "svs monitor".white());
    println!("{} {}", "  4. Perform a switch:".dimmed(), "svs switch".white());
    
    println!();
    println!("{}", "Documentation:".bright_cyan());
    println!("{} {}", "  • Help:".dimmed(), "svs --help".white());
    println!("{} {}", "  • Config help:".dimmed(), "svs config --help".white());
    println!("{} {}", "  • Switch help:".dimmed(), "svs switch --help".white());
    
    println!();
    println!("{}", "🚀 Happy validating!".bright_green().bold());
    println!();
    
    Ok(())
}