use aquatic_toml_config::TomlConfig;
use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, TomlConfig, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchConfig {
    pub num_rounds: usize,
    pub num_threads: usize,
    pub num_connect_requests: usize,
    pub num_announce_requests: usize,
    pub num_scrape_requests: usize,
    pub num_hashes_per_scrape_request: usize,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            num_rounds: 10,
            num_threads: 2,
            num_connect_requests: 5_000_000,
            num_announce_requests: 2_000_000,
            num_scrape_requests: 2_000_000,
            num_hashes_per_scrape_request: 20,
        }
    }
}

impl aquatic_common::cli::Config for BenchConfig {}

#[cfg(test)]
mod tests {
    use super::BenchConfig;

    ::aquatic_toml_config::gen_serialize_deserialize_test!(BenchConfig);
}
