use rand::{thread_rng, Rng};
use rand::distributions::Alphanumeric;

pub fn generate_slug(name: &str) -> String {
    // Normalize: lowercase, strip non-ASCII, keep only [a-z0-9-]
    let base: String = name
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii())
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();

    let trimmed = base
        .split('-')
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
