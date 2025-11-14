use std::fs::{File, read_to_string};
use std::io::Write;
use std::path::Path;

use crate::config::get_config;
use rusteze_common::DbConfig;

pub fn get_db_attributes(attr_str: String) -> String {
    let mut db_name = "unknown-database".to_string();

    for pair in attr_str.split(',') {
        let pair = pair.trim();

        if let Some((key, value)) = pair.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"').trim_matches('\'').trim();

            match key {
                "name" => db_name = value.to_string(),
                _ => {} // Ignore unknown.
            }
        }
    }

    return db_name;
}

pub fn update_db_manifest(manifest_path: &Path, db_name: &str) {
    let mut config = get_config(manifest_path);

    if config.db.is_none() {
        config.db = Some(std::collections::HashMap::new())
    }

    if let Some(ref mut db_config) = config.db {
        db_config.entry(db_name.to_string()).or_insert(DbConfig {
            name: db_name.to_string(),
            description: Some(format!("DB: {}", db_name)),
        });
    }

    // @todo - this should really use the try_lock
    let json_string = serde_json::to_string_pretty(&config).unwrap();
    let mut f = File::create(manifest_path).unwrap();
    f.write_all(json_string.as_bytes()).unwrap();
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_attributes() {
        let s = "name = 'users'".to_string();
        assert_eq!(get_db_attributes(s), "users")
    }
}
