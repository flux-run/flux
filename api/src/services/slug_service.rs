use rand::{thread_rng, Rng};
use rand::distributions::Alphanumeric;

pub fn generate_slug(name: &str) -> String {
    let base = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>();
    
    let trimmed = base.split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    if trimmed.is_empty() {
        return format!("{}-org", generate_random_suffix(8));
    }

    format!("{}-{}-org", trimmed, generate_random_suffix(6))
}

fn generate_random_suffix(len: usize) -> String {
    thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect::<String>()
        .to_lowercase()
}
