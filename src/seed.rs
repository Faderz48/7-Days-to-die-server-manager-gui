//! Random world seed generator. 7DTD's RWG accepts any string as a seed —
//! identical strings produce identical worlds. We generate readable
//! "name + adjective + number" combinations so they're easy to remember
//! and recognize when picking from a list.

use rand::seq::SliceRandom;
use rand::Rng;

const ADJECTIVES: &[&str] = &[
    "Ashen", "Bleak", "Crimson", "Dusty", "Echo", "Forsaken", "Grim", "Hollow",
    "Iron", "Jagged", "Kindred", "Lonely", "Misty", "Nightfall", "Old", "Pale",
    "Quiet", "Rusted", "Silent", "Tattered", "Umber", "Vacant", "Wretched",
    "Yonder", "Zenith", "Cinder", "Frozen", "Burnt", "Wild", "Howling",
];

const NOUNS: &[&str] = &[
    "Ridge", "Hollow", "Plains", "Reach", "Vale", "Drift", "Basin", "Pass",
    "Crossing", "Hill", "Marsh", "Pines", "Mesa", "Crater", "Forge", "Mill",
    "Outpost", "Falls", "Trace", "Bend", "Gulch", "Camp", "Reservoir",
    "Quarry", "Fields", "Highway", "Junction", "Spire", "Sanctum", "Refuge",
];

/// Generate a single readable seed string like "AshenRidge42".
pub fn generate_one() -> String {
    let mut rng = rand::thread_rng();
    let adj = ADJECTIVES.choose(&mut rng).copied().unwrap_or("Wild");
    let noun = NOUNS.choose(&mut rng).copied().unwrap_or("Reach");
    let num: u16 = rng.gen_range(1..=9999);
    format!("{adj}{noun}{num}")
}

/// Generate `n` distinct seed strings.
pub fn generate_many(n: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        let candidate = generate_one();
        if !out.contains(&candidate) {
            out.push(candidate);
        }
    }
    out
}
