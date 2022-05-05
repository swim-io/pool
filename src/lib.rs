pub mod amp_factor;
pub mod common;
pub mod decimal;
#[cfg(not(feature = "no-entrypoint"))]
pub mod entrypoint;
pub mod error;
pub mod instruction;
pub mod invariant;
pub mod pool_fee;
pub mod processor;
pub mod state;

pub const TOKEN_COUNT: usize = 6;

use solana_security_txt::security_txt;

security_txt! {
    // Required fields
    name: "Swim.io",
    project_url: "https://swim.io/",
    contacts: "email:admin@swim.io",
    policy: "https://swim.io/security",

    // Optional fields
    preferred_languages: "en",
    encryption: "https://swim.io/pgp-key.txt",
    expiry: "2026-04-28T05:00:00.000Z",
    auditors: "Kudelski"
}

// 4 Pool
//solana_program::declare_id!("SWiMBJS9iBU1rMLAKBVfp73ThW1xPPwKdBHEU2JFpuo");

// 6 Pool
solana_program::declare_id!("SWiMDJYFUGj6cPrQ6QYYYWZtvXQdRChSVAygDZDsCHC");
