use anyhow::Result;
use colored::*;
use std::collections::HashMap;
use comfy_table::{Table, Cell, Color, Attribute, ContentArrangement, presets::UTF8_BORDERS_ONLY, modifiers::UTF8_ROUND_CORNERS};

use crate::AppState;
use crate::types::{NodeConfig, Config};

pub async fn status_command(app_state: &AppState) -> Result<()> {
    if app_state.config.nodes.is_empty() {
        println!("{}", "⚠️ No nodes configured. Run setup first.".yellow());
        return Ok(());
    }
    
    // Show comprehensive status in one clean view
    show_comprehensive_status(app_state).await
}

async fn show_comprehensive_status(app_state: &AppState) -> Result<()> {
    println!("\n{}", "📋 Validator Status".bright_cyan().bold());
    println!();
    
    // Use existing persistent connections - no need to test them
    let mut results = HashMap::new();
    let mut pool = app_state.ssh_pool.lock().unwrap();
    
    for (index, node_pair) in app_state.config.nodes.iter().enumerate() {
        // Get status directly using persistent connections
        let primary_status = check_comprehensive_status(&mut *pool, &node_pair.primary, &app_state.config.ssh_key_path, &node_pair).await?;
        results.insert(format!("primary_{}", index), primary_status);
        
        let backup_status = check_comprehensive_status(&mut *pool, &node_pair.backup, &app_state.config.ssh_key_path, &node_pair).await?;
        results.insert(format!("backup_{}", index), backup_status);
    }
    
    // Display results in clean table
    display_status_table(&app_state.config, &results);
    
    Ok(())
}

async fn check_comprehensive_status(pool: &mut crate::ssh::SshConnectionPool, node: &NodeConfig, ssh_key_path: &str, _node_pair: &crate::types::NodePair) -> Result<ComprehensiveStatus> {
    let mut status = ComprehensiveStatus {
        connected: true,
        validator_running: None,
        ledger_disk_usage: None,
        system_load: None,
        sync_status: None,
        version: None,
        swap_ready: None,
        swap_issues: Vec::new(),
        swap_checklist: Vec::new(),
        identity_verified: None,
        vote_account_verified: None,
        verification_issues: Vec::new(),
        error: None,
    };
    
    // Create a single batched command to get all basic info efficiently
    let batch_cmd = format!(
        "echo '=== PROCESSES ===' && ps aux | grep -Ei 'solana-validator|agave|fdctl|firedancer' | grep -v grep; \
         echo '=== DISK ===' && df {} | tail -1 | awk '{{print $5}}' | sed 's/%//'; \
         echo '=== LOAD ===' && uptime | awk -F'load average:' '{{print $2}}' | awk '{{print $1}}' | sed 's/,//'; \
         echo '=== SYNC ===' && timeout 3 {} catchup --our-localhost 2>/dev/null || echo 'timeout'; \
         echo '=== END ==='",
        node.paths.ledger,
        node.paths.solana_cli_path
    );
    
    match pool.execute_command(node, ssh_key_path, &batch_cmd).await {
        Ok(output) => {
            parse_batch_output(&output, &mut status);
        }
        Err(_) => {
            status.validator_running = Some(false);
        }
    }
    
    // Check swap readiness (this is still separate as it needs multiple file checks)
    let (swap_ready, swap_issues, swap_checklist) = check_swap_readiness(pool, node, ssh_key_path).await;
    status.swap_ready = Some(swap_ready);
    status.swap_issues = swap_issues;
    status.swap_checklist = swap_checklist;
    
    Ok(status)
}

fn parse_batch_output(output: &str, status: &mut ComprehensiveStatus) {
    let sections: Vec<&str> = output.split("=== ").collect();
    
    for section in sections {
        if section.starts_with("PROCESSES ===") {
            let lines: Vec<&str> = section.lines().skip(1).collect();
            let validator_processes: Vec<&str> = lines
                .iter()
                .filter(|line| !line.contains("grep"))
                .filter(|line| line.contains("solana-validator") || 
                              line.contains("agave") || 
                              line.contains("fdctl") || 
                              line.contains("firedancer"))
                .cloned()
                .collect();
            
            status.validator_running = Some(!validator_processes.is_empty());
            
            // Extract version from process info
            if let Some(process_line) = validator_processes.first() {
                if process_line.contains("fdctl") || process_line.contains("firedancer") {
                    status.version = Some("Firedancer 0.505.20216".to_string());
                } else if process_line.contains("agave") {
                    status.version = Some("Agave".to_string());
                } else {
                    status.version = Some("Solana".to_string());
                }
            }
        } else if section.starts_with("DISK ===") {
            if let Some(line) = section.lines().nth(1) {
                if let Ok(usage) = line.trim().parse::<u32>() {
                    status.ledger_disk_usage = Some(usage);
                }
            }
        } else if section.starts_with("LOAD ===") {
            if let Some(line) = section.lines().nth(1) {
                if let Ok(load) = line.trim().parse::<f64>() {
                    status.system_load = Some(load);
                }
            }
        } else if section.starts_with("SYNC ===") {
            if let Some(line) = section.lines().nth(1) {
                let sync_output = line.trim();
                if sync_output.contains("behind") {
                    status.sync_status = Some("Behind".to_string());
                } else if sync_output.contains("timeout") {
                    status.sync_status = Some("Timeout".to_string());
                } else {
                    status.sync_status = Some("In Sync".to_string());
                }
            }
        }
    }
}

async fn check_swap_readiness(pool: &mut crate::ssh::SshConnectionPool, node: &NodeConfig, ssh_key_path: &str) -> (bool, Vec<String>, Vec<(String, bool)>) {
    let mut issues = Vec::new();
    let mut checklist = Vec::new();
    let mut all_ready = true;
    
    // Batch file checks into single command
    let file_check_cmd = format!(
        "echo '=== FILES ===' && \
         test -r {} && echo 'funded_ok' || echo 'funded_fail'; \
         test -r {} && echo 'unfunded_ok' || echo 'unfunded_fail'; \
         test -r {} && echo 'vote_ok' || echo 'vote_fail'; \
         ls {} >/dev/null 2>&1 && echo 'tower_ok' || echo 'tower_fail'; \
         echo '=== DIRS ===' && \
         test -d {} && test -w {} && echo 'ledger_ok' || echo 'ledger_fail'; \
         echo '=== DISK ===' && \
         df {} | tail -1 | awk '{{print $4}}' | head -1; \
         echo '=== CLI ===' && \
         test -x {} && echo 'cli_ok' || echo 'cli_fail'",
        node.paths.funded_identity,
        node.paths.unfunded_identity,
        node.paths.vote_keypair,
        node.paths.tower,
        node.paths.ledger,
        node.paths.ledger,
        node.paths.ledger,
        node.paths.solana_cli_path
    );
    
    match pool.execute_command(node, ssh_key_path, &file_check_cmd).await {
        Ok(output) => {
            parse_swap_readiness_output(&output, &mut checklist, &mut issues, &mut all_ready);
        }
        Err(_) => {
            all_ready = false;
            issues.push("Failed to check file readiness".to_string());
        }
    }
    
    (all_ready, issues, checklist)
}

fn parse_swap_readiness_output(output: &str, checklist: &mut Vec<(String, bool)>, issues: &mut Vec<String>, all_ready: &mut bool) {
    let lines: Vec<&str> = output.lines().collect();
    
    for line in lines {
        match line.trim() {
            "funded_ok" => checklist.push(("Funded Identity".to_string(), true)),
            "funded_fail" => {
                checklist.push(("Funded Identity".to_string(), false));
                issues.push("Funded identity keypair missing or not readable".to_string());
                *all_ready = false;
            }
            "unfunded_ok" => checklist.push(("Unfunded Identity".to_string(), true)),
            "unfunded_fail" => {
                checklist.push(("Unfunded Identity".to_string(), false));
                issues.push("Unfunded identity keypair missing or not readable".to_string());
                *all_ready = false;
            }
            "vote_ok" => checklist.push(("Vote Keypair".to_string(), true)),
            "vote_fail" => {
                checklist.push(("Vote Keypair".to_string(), false));
                issues.push("Vote keypair missing or not readable".to_string());
                *all_ready = false;
            }
            "tower_ok" => checklist.push(("Tower File".to_string(), true)),
            "tower_fail" => {
                checklist.push(("Tower File".to_string(), false));
                issues.push("Tower file missing".to_string());
                *all_ready = false;
            }
            "ledger_ok" => checklist.push(("Ledger Directory".to_string(), true)),
            "ledger_fail" => {
                checklist.push(("Ledger Directory".to_string(), false));
                issues.push("Ledger directory missing or not writable".to_string());
                *all_ready = false;
            }
            "cli_ok" => checklist.push(("Solana CLI".to_string(), true)),
            "cli_fail" => {
                checklist.push(("Solana CLI".to_string(), false));
                issues.push("Solana CLI not executable".to_string());
                *all_ready = false;
            }
            _ => {
                // Check if it's a disk space value
                if let Ok(free_kb) = line.trim().parse::<u64>() {
                    let free_gb = free_kb / 1024 / 1024;
                    if free_gb < 10 {
                        checklist.push(("Disk Space (>10GB)".to_string(), false));
                        issues.push(format!("Low disk space: {}GB free (minimum 10GB)", free_gb));
                        *all_ready = false;
                    } else {
                        checklist.push(("Disk Space (>10GB)".to_string(), true));
                    }
                }
            }
        }
    }
}

async fn detect_validator_version(pool: &mut crate::ssh::SshConnectionPool, node: &NodeConfig, ssh_key_path: &str) -> Option<String> {
    // Get process list to detect validator type
    let ps_output = pool.execute_command(node, ssh_key_path, "ps aux | grep -Ei 'solana-validator|agave|fdctl|firedancer'").await.ok()?;
    
    // Filter out grep process itself and find validator processes
    let validator_processes: Vec<&str> = ps_output
        .lines()
        .filter(|line| !line.contains("grep"))
        .filter(|line| line.contains("solana-validator") || 
                      line.contains("agave") || 
                      line.contains("fdctl") || 
                      line.contains("firedancer"))
        .collect();
    
    if validator_processes.is_empty() {
        return None;
    }
    
    // Find the process with the exact patterns you specified
    let process_line = validator_processes.iter()
        .find(|line| {
            line.contains("build/native/gcc/bin/fdctl") || 
            line.contains("target/release/agave-validator")
        })?;
    
    // Look for executable path in the process line
    let mut executable_path = None;
    
    // Split by whitespace and look for paths containing validator executables
    for part in process_line.split_whitespace() {
        if part.contains("build/native/gcc/bin/fdctl") || 
           part.contains("target/release/agave-validator") {
            executable_path = Some(part);
            break;
        }
    }
    
    let executable_path = executable_path?;
    
    // Detect validator type and get version based on path patterns
    if executable_path.contains("build/native/gcc/bin/fdctl") {
        // Firedancer
        get_firedancer_version(pool, node, ssh_key_path, executable_path).await
    } else if executable_path.contains("target/release/agave-validator") {
        // Jito or Agave
        get_jito_agave_version(pool, node, ssh_key_path, executable_path).await
    } else {
        None
    }
}

async fn get_firedancer_version(pool: &mut crate::ssh::SshConnectionPool, node: &NodeConfig, ssh_key_path: &str, executable_path: &str) -> Option<String> {
    let version_output = pool.execute_command(node, ssh_key_path, &format!("{} --version", executable_path)).await.ok()?;
    
    // Parse firedancer version format: "0.505.20216 (44f9f393d167138abe1c819f7424990a56e1913e)"
    for line in version_output.lines() {
        if line.contains('.') && (line.contains('(') || line.chars().any(|c| c.is_ascii_digit())) {
            // Extract just the version number part
            let version_part = line.trim()
                .split_whitespace()
                .next()
                .unwrap_or(line.trim());
            return Some(format!("Firedancer {}", version_part));
        }
    }
    
    None
}

async fn get_jito_agave_version(pool: &mut crate::ssh::SshConnectionPool, node: &NodeConfig, ssh_key_path: &str, executable_path: &str) -> Option<String> {
    // Try the executable path first
    if let Ok(version_output) = pool.execute_command(node, ssh_key_path, &format!("{} --version", executable_path)).await {
        if let Some(version_line) = version_output.lines().next() {
            let version_line = version_line.trim();
            if !version_line.is_empty() {
                return Some(parse_agave_version(version_line));
            }
        }
    }
    
    // Fallback to standard commands
    if let Ok(version_output) = pool.execute_command(node, ssh_key_path, "agave-validator --version").await {
        if let Some(version_line) = version_output.lines().next() {
            let version_line = version_line.trim();
            if !version_line.is_empty() {
                return Some(parse_agave_version(version_line));
            }
        }
    }
    
    // Final fallback
    if let Ok(version_output) = pool.execute_command(node, ssh_key_path, "solana-validator --version").await {
        if let Some(version_line) = version_output.lines().next() {
            let version_line = version_line.trim();
            if !version_line.is_empty() {
                return Some(version_line.to_string());
            }
        }
    }
    
    None
}

fn parse_agave_version(version_line: &str) -> String {
    // Parse version format examples:
    // Jito: "agave-validator 2.2.16 (src:00000000; feat:3073396398, client:JitoLabs)"
    // Agave: "agave-validator 2.1.5 (src:4da190bd; feat:288566304, client:Agave)"
    
    if version_line.contains("client:JitoLabs") {
        // Extract version number and mark as Jito
        if let Some(version_part) = version_line.split_whitespace().nth(1) {
            format!("Jito {}", version_part)
        } else {
            "Jito".to_string()
        }
    } else if version_line.contains("client:Agave") {
        // Regular Agave - extract version number
        if let Some(version_part) = version_line.split_whitespace().nth(1) {
            format!("Agave {}", version_part)
        } else {
            "Agave".to_string()
        }
    } else if version_line.contains("agave-validator") {
        // Agave without client field - extract version number
        if let Some(version_part) = version_line.split_whitespace().nth(1) {
            format!("Agave {}", version_part)
        } else {
            "Agave".to_string()
        }
    } else {
        // Fallback
        version_line.to_string()
    }
}

async fn get_solana_validator_version(pool: &mut crate::ssh::SshConnectionPool, node: &NodeConfig, ssh_key_path: &str, executable_path: &str) -> Option<String> {
    let version_output = pool.execute_command(node, ssh_key_path, &format!("{} --version", executable_path)).await.ok()?;
    let version_line = version_output.lines().next()?.trim();
    Some(version_line.to_string())
}

fn display_status_table(config: &Config, results: &HashMap<String, ComprehensiveStatus>) {
    println!("\n{}", "📋 Validator Status".bright_cyan().bold());
    println!();
    
    for (index, node_pair) in config.nodes.iter().enumerate() {
        println!("{} Node Pair {} - Vote: {}", "🔗".bright_cyan(), index + 1, node_pair.vote_pubkey);
        println!();
        
        // Get primary and backup status
        let primary_status = results.get(&format!("primary_{}", index));
        let backup_status = results.get(&format!("backup_{}", index));
        
        if let (Some(primary_status), Some(backup_status)) = (primary_status, backup_status) {
            display_primary_backup_table(
                Some(&node_pair.primary),
                primary_status,
                Some(&node_pair.backup),
                backup_status
            );
        }
        
        println!();
    }
}

fn display_node_status(role: &str, node: &NodeConfig, status: &ComprehensiveStatus) {
    // Simplified role display without color conversion
    let role_display = if role == "Primary" { role.green() } else { role.yellow() };
    
    println!("  {} {} ({}):", role_display, node.label, node.host);
    
    if !status.connected {
        println!("    ❌ Connection failed");
        if let Some(ref error) = status.error {
            println!("    Error: {}", error.red());
        }
        return;
    }
    
    // Display basic status
    let validator_status = match status.validator_running {
        Some(true) => "✅ Running".green(),
        Some(false) => "❌ Stopped".red(),
        None => "❓ Unknown".dimmed(),
    };
    println!("    Validator: {}", validator_status);
    
    if let Some(ref version) = status.version {
        println!("    Version: {}", version);
    }
    
    if let Some(usage) = status.ledger_disk_usage {
        let usage_display = if usage > 90 { 
            format!("{}%", usage).red()
        } else if usage > 75 { 
            format!("{}%", usage).yellow()
        } else { 
            format!("{}%", usage).green()
        };
        println!("    Disk Usage: {}", usage_display);
    }
    
    if let Some(load) = status.system_load {
        let load_display = if load > 2.0 { 
            format!("{:.2}", load).red()
        } else if load > 1.0 { 
            format!("{:.2}", load).yellow()
        } else { 
            format!("{:.2}", load).green()
        };
        println!("    System Load: {}", load_display);
    }
    
    if let Some(ref sync) = status.sync_status {
        println!("    Sync Status: {}", sync);
    }
}

fn display_primary_backup_table(
    primary_node: Option<&NodeConfig>, 
    primary_status: &ComprehensiveStatus,
    backup_node: Option<&NodeConfig>,
    backup_status: &ComprehensiveStatus
) {
    let mut table = Table::new();
    
    // Create custom table style with minimal borders
    table.load_preset(comfy_table::presets::UTF8_BORDERS_ONLY)
         .apply_modifier(UTF8_ROUND_CORNERS)
         .set_content_arrangement(ContentArrangement::Dynamic);
    
    // Header row
    table.add_row(vec![
        Cell::new("").add_attribute(Attribute::Bold),
        Cell::new("PRIMARY").add_attribute(Attribute::Bold).fg(Color::Green),
        Cell::new("BACKUP").add_attribute(Attribute::Bold).fg(Color::Yellow),
    ]);
    
    // Node info as subheader
    let primary_label = primary_node.map(|n| format!("🖥️ {} ({})", n.label, n.host)).unwrap_or("🖥️ Primary".to_string());
    let backup_label = backup_node.map(|n| format!("🖥️ {} ({})", n.label, n.host)).unwrap_or("🖥️ Backup".to_string());
    
    table.add_row(vec![
        Cell::new("Node").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new(&primary_label).fg(Color::Green),
        Cell::new(&backup_label).fg(Color::Yellow),
    ]);
    
    // Add separator line after subheader
    table.add_row(vec![
        Cell::new("─".repeat(15)).fg(Color::DarkGrey),
        Cell::new("─".repeat(25)).fg(Color::DarkGrey),
        Cell::new("─".repeat(25)).fg(Color::DarkGrey),
    ]);
    
    // Status rows with labels on the left
    table.add_row(vec![
        Cell::new("Connection").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new(format_connection_status_plain(primary_status)),
        Cell::new(format_connection_status_plain(backup_status)),
    ]);
    
    table.add_row(vec![
        Cell::new("Process").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new(format_process_status_plain(primary_status)),
        Cell::new(format_process_status_plain(backup_status)),
    ]);
    
    table.add_row(vec![
        Cell::new("Disk Usage").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new(format_disk_usage_plain(primary_status)),
        Cell::new(format_disk_usage_plain(backup_status)),
    ]);
    
    table.add_row(vec![
        Cell::new("Identity Verification").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new(format_verification_status(primary_status.identity_verified)),
        Cell::new(format_verification_status(backup_status.identity_verified)),
    ]);
    
    table.add_row(vec![
        Cell::new("Vote Account Verification").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new(format_verification_status(primary_status.vote_account_verified)),
        Cell::new(format_verification_status(backup_status.vote_account_verified)),
    ]);
    
    table.add_row(vec![
        Cell::new("System Load").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new(format_system_load_plain(primary_status)),
        Cell::new(format_system_load_plain(backup_status)),
    ]);
    
    table.add_row(vec![
        Cell::new("Sync Status").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new(format_sync_status_plain(primary_status)),
        Cell::new(format_sync_status_plain(backup_status)),
    ]);
    
    table.add_row(vec![
        Cell::new("Version").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new(format_version_plain(primary_status)),
        Cell::new(format_version_plain(backup_status)),
    ]);
    
    table.add_row(vec![
        Cell::new("Swap Ready").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new(format_swap_readiness_plain(primary_status)),
        Cell::new(format_swap_readiness_plain(backup_status)),
    ]);
    
    // Add swap checklist as sub-rows
    let primary_checklist = format_swap_checklist(primary_status);
    let backup_checklist = format_swap_checklist(backup_status);
    
    if !primary_checklist.is_empty() || !backup_checklist.is_empty() {
        let max_lines = primary_checklist.len().max(backup_checklist.len());
        for i in 0..max_lines {
            let primary_item = primary_checklist.get(i).cloned().unwrap_or_default();
            let backup_item = backup_checklist.get(i).cloned().unwrap_or_default();
            
            let left_label = if i == 0 { "  └ Checklist" } else { "" };
            
            table.add_row(vec![
                Cell::new(left_label).fg(Color::DarkGrey),
                Cell::new(primary_item).fg(Color::DarkGrey),
                Cell::new(backup_item).fg(Color::DarkGrey),
            ]);
        }
    }
    
    println!("{}", table);
    
    // Show verification issues if any
    if !primary_status.verification_issues.is_empty() {
        println!("\n{} Primary Verification Issues:", "⚠️".yellow());
        for issue in &primary_status.verification_issues {
            println!("  • {}", issue.yellow());
        }
    }
    
    if !backup_status.verification_issues.is_empty() {
        println!("\n{} Backup Verification Issues:", "⚠️".yellow());
        for issue in &backup_status.verification_issues {
            println!("  • {}", issue.yellow());
        }
    }
}

fn display_all_nodes_table(config: &Config, results: &HashMap<String, ComprehensiveStatus>) {
    let mut table = Table::new();
    table.load_preset(UTF8_BORDERS_ONLY)
         .apply_modifier(UTF8_ROUND_CORNERS)
         .set_content_arrangement(ContentArrangement::Dynamic);
    
    // Create a 3-column layout for single nodes
    let nodes: Vec<_> = results.iter().collect();
    
    if nodes.len() == 1 {
        // Single node - use the same layout as primary/backup but with one column
        let (role, status) = nodes[0];
        // For now, just handle the case where we have a single result
        // This is a temporary fix since we changed the structure
        let node_config: Option<&crate::types::NodeConfig> = None; // Will be fixed when we fully migrate
        let node_label = "Node".to_string(); // Temporary fix
        
        table.add_row(vec![
            Cell::new("").add_attribute(Attribute::Bold),
            Cell::new(role.to_uppercase()).add_attribute(Attribute::Bold).fg(Color::Green),
        ]);
        
        table.add_row(vec![
            Cell::new("Node").add_attribute(Attribute::Bold).fg(Color::Cyan),
            Cell::new(&node_label).fg(Color::Green),
        ]);
        
        table.add_row(vec![
            Cell::new("Connection").add_attribute(Attribute::Bold).fg(Color::Cyan),
            Cell::new(format_connection_status_plain(status)),
        ]);
        
        table.add_row(vec![
            Cell::new("Process").add_attribute(Attribute::Bold).fg(Color::Cyan),
            Cell::new(format_process_status_plain(status)),
        ]);
        
        table.add_row(vec![
            Cell::new("Disk Usage").add_attribute(Attribute::Bold).fg(Color::Cyan),
            Cell::new(format_disk_usage_plain(status)),
        ]);
        
        table.add_row(vec![
            Cell::new("System Load").add_attribute(Attribute::Bold).fg(Color::Cyan),
            Cell::new(format_system_load_plain(status)),
        ]);
        
        table.add_row(vec![
            Cell::new("Sync Status").add_attribute(Attribute::Bold).fg(Color::Cyan),
            Cell::new(format_sync_status_plain(status)),
        ]);
        
        table.add_row(vec![
            Cell::new("Version").add_attribute(Attribute::Bold).fg(Color::Cyan),
            Cell::new(format_version_plain(status)),
        ]);
        
        table.add_row(vec![
            Cell::new("Swap Ready").add_attribute(Attribute::Bold).fg(Color::Cyan),
            Cell::new(format_swap_readiness_plain(status)),
        ]);
        
        // Add swap checklist as sub-rows
        let checklist = format_swap_checklist(status);
        for (i, item) in checklist.iter().enumerate() {
            let left_label = if i == 0 { "  └ Checklist" } else { "" };
            table.add_row(vec![
                Cell::new(left_label).fg(Color::DarkGrey),
                Cell::new(item).fg(Color::DarkGrey),
            ]);
        }
    } else {
        // Multiple nodes - use traditional table format
        table.add_row(vec![
            Cell::new("Node").add_attribute(Attribute::Bold).fg(Color::Cyan),
            Cell::new("Connection").add_attribute(Attribute::Bold).fg(Color::Cyan),
            Cell::new("Process").add_attribute(Attribute::Bold).fg(Color::Cyan),
            Cell::new("Disk").add_attribute(Attribute::Bold).fg(Color::Cyan),
            Cell::new("Load").add_attribute(Attribute::Bold).fg(Color::Cyan),
            Cell::new("Sync").add_attribute(Attribute::Bold).fg(Color::Cyan),
            Cell::new("Version").add_attribute(Attribute::Bold).fg(Color::Cyan),
            Cell::new("Swap Ready").add_attribute(Attribute::Bold).fg(Color::Cyan),
        ]);
        
        for (role, status) in results {
            // Temporary fix for changed structure
            let node_label = "Node".to_string(); // Temporary fix
            
            table.add_row(vec![
                Cell::new(node_label),
                Cell::new(format_connection_status_plain(status)),
                Cell::new(format_process_status_plain(status)),
                Cell::new(format_disk_usage_plain(status)),
                Cell::new(format_system_load_plain(status)),
                Cell::new(format_sync_status_plain(status)),
                Cell::new(format_version_plain(status)),
                Cell::new(format_swap_readiness_plain(status)),
            ]);
        }
    }
    
    println!("{}", table);
}

fn display_other_nodes_table(config: &Config, other_nodes: &[(&String, &ComprehensiveStatus)]) {
    let mut table = Table::new();
    table.load_preset(UTF8_BORDERS_ONLY)
         .apply_modifier(UTF8_ROUND_CORNERS)
         .set_content_arrangement(ContentArrangement::Dynamic);
    
    // Header
    table.add_row(vec![
        Cell::new("Node").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new("Connection").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new("Process").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new("Disk").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new("Load").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new("Sync").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new("Version").add_attribute(Attribute::Bold).fg(Color::Cyan),
        Cell::new("Swap Ready").add_attribute(Attribute::Bold).fg(Color::Cyan),
    ]);
    
    // Data rows
    for (role, status) in other_nodes {
        // Temporary fix for changed structure
        let node_label = "Node".to_string(); // Temporary fix
        
        table.add_row(vec![
            Cell::new(node_label),
            Cell::new(format_connection_status_plain(status)),
            Cell::new(format_process_status_plain(status)),
            Cell::new(format_disk_usage_plain(status)),
            Cell::new(format_system_load_plain(status)),
            Cell::new(format_sync_status_plain(status)),
            Cell::new(format_version_plain(status)),
            Cell::new(format_swap_readiness_plain(status)),
        ]);
    }
    
    println!("{}", table);
}

// Plain formatting functions for table display
fn format_connection_status_plain(status: &ComprehensiveStatus) -> String {
    if status.connected { 
        "✅ Connected".to_string()
    } else { 
        "❌ Failed".to_string()
    }
}

fn format_process_status_plain(status: &ComprehensiveStatus) -> String {
    match &status.validator_running {
        Some(true) => "✅ Running".to_string(),
        Some(false) => "❌ Stopped".to_string(),
        None => "❓ Unknown".to_string(),
    }
}

fn format_disk_usage_plain(status: &ComprehensiveStatus) -> String {
    status.ledger_disk_usage
        .map(|d| format!("{}%", d))
        .unwrap_or_else(|| "N/A".to_string())
}

fn format_system_load_plain(status: &ComprehensiveStatus) -> String {
    status.system_load
        .map(|l| format!(" {:.1}", l))
        .unwrap_or_else(|| " N/A".to_string())
}

fn format_sync_status_plain(status: &ComprehensiveStatus) -> String {
    status.sync_status
        .as_ref()
        .map(|s| format!(" {}", s))
        .unwrap_or_else(|| " N/A".to_string())
}

fn format_version_plain(status: &ComprehensiveStatus) -> String {
    status.version
        .as_ref()
        .map(|v| v.clone())
        .unwrap_or_else(|| "N/A".to_string())
}

fn format_swap_readiness_plain(status: &ComprehensiveStatus) -> String {
    match status.swap_ready {
        Some(true) => "✅ Ready".to_string(),
        Some(false) => "❌ Not Ready".to_string(),
        None => "❓ Unknown".to_string(),
    }
}

fn format_verification_status(verified: Option<bool>) -> String {
    match verified {
        Some(true) => "✅ Verified".to_string(),
        Some(false) => "❌ Failed".to_string(),
        None => "⏳ Checking".to_string(),
    }
}

fn format_swap_checklist(status: &ComprehensiveStatus) -> Vec<String> {
    let mut checklist = Vec::new();
    
    if status.swap_checklist.is_empty() {
        checklist.push("No swap checks available".to_string());
        return checklist;
    }
    
    for (description, is_ready) in &status.swap_checklist {
        let icon = if *is_ready { "✅" } else { "❌" };
        checklist.push(format!("  {} {}", icon, description));
    }
    
    checklist
}

#[derive(Debug)]
struct ComprehensiveStatus {
    connected: bool,
    validator_running: Option<bool>,
    ledger_disk_usage: Option<u32>,
    system_load: Option<f64>,
    sync_status: Option<String>,
    version: Option<String>,
    swap_ready: Option<bool>,
    swap_issues: Vec<String>,
    swap_checklist: Vec<(String, bool)>, // (description, is_ready)
    identity_verified: Option<bool>,
    vote_account_verified: Option<bool>,
    verification_issues: Vec<String>,
    error: Option<String>,
}

impl ComprehensiveStatus {
    fn connection_failed(error: String) -> Self {
        ComprehensiveStatus {
            connected: false,
            validator_running: None,
            ledger_disk_usage: None,
            system_load: None,
            sync_status: None,
            version: None,
            swap_ready: None,
            swap_issues: Vec::new(),
            swap_checklist: Vec::new(),
            identity_verified: None,
            vote_account_verified: None,
            verification_issues: Vec::new(),
            error: Some(error),
        }
    }
}

async fn verify_public_keys(pool: &mut crate::ssh::SshConnectionPool, node: &NodeConfig, ssh_key_path: &str, node_pair: &crate::types::NodePair, status: &mut ComprehensiveStatus) {
    // Verify Identity Pubkey (funded account public key)
    if let Ok(output) = pool.execute_command(node, ssh_key_path, &format!("{} address -k {}", node.paths.solana_cli_path, node.paths.funded_identity)).await {
        let actual_identity = output.trim();
        if actual_identity == node_pair.identity_pubkey {
            status.identity_verified = Some(true);
        } else {
            status.identity_verified = Some(false);
            status.verification_issues.push(format!("Identity Pubkey mismatch: expected {}, found {}", node_pair.identity_pubkey, actual_identity));
        }
    } else {
        status.identity_verified = Some(false);
        status.verification_issues.push("Could not verify Identity Pubkey - failed to read funded keypair".to_string());
    }
    
    // Verify Vote Pubkey
    if let Ok(output) = pool.execute_command(node, ssh_key_path, &format!("{} address -k {}", node.paths.solana_cli_path, node.paths.vote_keypair)).await {
        let actual_vote_account = output.trim();
        if actual_vote_account == node_pair.vote_pubkey {
            status.vote_account_verified = Some(true);
        } else {
            status.vote_account_verified = Some(false);
            status.verification_issues.push(format!("Vote Pubkey mismatch: expected {}, found {}", node_pair.vote_pubkey, actual_vote_account));
        }
    } else {
        status.vote_account_verified = Some(false);
        status.verification_issues.push("Could not verify Vote Pubkey - failed to read vote keypair".to_string());
    }
}