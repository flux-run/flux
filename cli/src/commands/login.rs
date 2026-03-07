use crate::api::config::Config;
use std::io::{self, Write};

pub async fn execute() {
    println!("Welcome to Fluxbase CLI!");
    
    // Prompt for Project ID
    print!("Project ID: ");
    io::stdout().flush().unwrap();
    
    let mut project_id = String::new();
    io::stdin().read_line(&mut project_id).unwrap();
    let project_id = project_id.trim().to_string();

    // Prompt securely for API Key without echoing
    let api_key = rpassword::prompt_password("API Key: ").unwrap();
    let api_key = api_key.trim().to_string();

    if !api_key.starts_with("flux_") {
        eprintln!("Error: Invalid API Key format. Keys must begin with 'flux_'");
        std::process::exit(1);
    }

    let mut config = Config::load().await;
    config.api_key = Some(api_key);
    config.project_id = Some(project_id);

    match config.save().await {
        Ok(_) => println!("Successfully authenticated CLI session!"),
        Err(e) => eprintln!("Error saving authentication settings: {}", e),
    }
}
