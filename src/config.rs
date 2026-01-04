// Configuration module
// Internal representation of user configuration

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Config {
    pub keybindings: HashMap<String, String>,
    pub settings: HashMap<String, ConfigValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfigValue {
    Bool(bool),
    Int(i64),
    String(String),
}

impl Config {
    pub fn default() -> Self {
        Self {
            keybindings: HashMap::new(),
            settings: HashMap::new(),
        }
    }

    /// Bind a key sequence to a command
    pub fn bind(&mut self, key: &str, command: &str) {
        self.keybindings
            .insert(key.to_string(), command.to_string());
    }

    /// Set a configuration value
    pub fn set<V: Into<ConfigValue>>(&mut self, key: &str, value: V) {
        self.settings.insert(key.to_string(), value.into());
    }

    /// Get a setting value (Test helper)
    #[cfg(test)]
    pub fn get(&self, key: &str) -> Option<&ConfigValue> {
        self.settings.get(key)
    }

    /// Get boolean setting (Test helper)
    #[cfg(test)]
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.get(key).and_then(|v| match v {
            ConfigValue::Bool(b) => Some(*b),
            _ => None,
        })
    }

    /// Get integer setting (Test helper)
    #[cfg(test)]
    pub fn get_int(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(|v| match v {
            ConfigValue::Int(i) => Some(*i),
            _ => None,
        })
    }

    /// Get string setting (Test helper)
    #[cfg(test)]
    pub fn get_string(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(|v| match v {
            ConfigValue::String(s) => Some(s.as_str()),
            _ => None,
        })
    }
}

impl From<bool> for ConfigValue {
    fn from(b: bool) -> Self {
        ConfigValue::Bool(b)
    }
}

impl From<i64> for ConfigValue {
    fn from(i: i64) -> Self {
        ConfigValue::Int(i)
    }
}

impl From<&str> for ConfigValue {
    fn from(s: &str) -> Self {
        ConfigValue::String(s.to_string())
    }
}

impl From<String> for ConfigValue {
    fn from(s: String) -> Self {
        ConfigValue::String(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = Config::default();
        assert!(config.keybindings.is_empty());
        assert!(config.settings.is_empty());
    }

    #[test]
    fn test_bind_key() {
        let mut config = Config::default();
        config.bind("^C", "copy");
        assert_eq!(config.keybindings.get("^C"), Some(&"copy".to_string()));
    }

    #[test]
    fn test_set_get_settings() {
        let mut config = Config::default();

        config.set("bool_setting", true);
        assert_eq!(config.get_bool("bool_setting"), Some(true));

        config.set("int_setting", 42);
        assert_eq!(config.get_int("int_setting"), Some(42));

        config.set("string_setting", "hello");
        assert_eq!(config.get_string("string_setting"), Some("hello"));
    }

    #[test]
    fn test_type_mismatch() {
        let mut config = Config::default();
        config.set("val", 10);
        // Should return None if type doesn't match
        assert_eq!(config.get_bool("val"), None);
        assert_eq!(config.get_string("val"), None);
    }

    #[test]
    fn test_config_value_conversions() {
        let b: ConfigValue = true.into();
        assert_eq!(b, ConfigValue::Bool(true));

        let i: ConfigValue = 100i64.into();
        assert_eq!(i, ConfigValue::Int(100));

        let s: ConfigValue = "test".into();
        assert_eq!(s, ConfigValue::String("test".to_string()));

        let s2: ConfigValue = String::from("test2").into();
        assert_eq!(s2, ConfigValue::String("test2".to_string()));
    }
}
