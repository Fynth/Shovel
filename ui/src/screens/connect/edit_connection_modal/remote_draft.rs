use models::{
    ClickHouseFormData,
    ConnectionRequest,
    MySqlFormData,
    PostgresFormData,
    SshTunnelConfig,
};

/// Editable copy of a remote connection's fields, held as strings so the form
/// can bind directly to inputs and only parse/validate on submit.
#[derive(Clone, PartialEq)]
pub struct RemoteConnectionDraft {
    pub host: String,
    pub port: String,
    pub username: String,
    pub password: String,
    pub database: String,
    pub ssl_mode: String,
    pub ssh_enabled: bool,
    pub ssh_host: String,
    pub ssh_port: String,
    pub ssh_username: String,
    pub ssh_private_key_path: String,
}

fn ssh_fields_from_tunnel(
    tunnel: &Option<SshTunnelConfig>,
) -> (bool, String, String, String, String) {
    match tunnel {
        Some(ssh) => (
            true,
            ssh.host.clone(),
            ssh.port.to_string(),
            ssh.username.clone(),
            ssh.private_key_path.clone(),
        ),
        None => (
            false,
            String::new(),
            "22".to_string(),
            String::new(),
            String::new(),
        ),
    }
}

impl RemoteConnectionDraft {
    pub fn postgres_default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: "5432".to_string(),
            username: "postgres".to_string(),
            ssl_mode: "prefer".to_string(),
            password: String::new(),
            database: "postgres".to_string(),
            ssh_enabled: false,
            ssh_host: String::new(),
            ssh_port: "22".to_string(),
            ssh_username: String::new(),
            ssh_private_key_path: String::new(),
        }
    }

    pub fn clickhouse_default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: "8123".to_string(),
            username: "default".to_string(),
            ssl_mode: String::new(),
            password: String::new(),
            database: "default".to_string(),
            ssh_enabled: false,
            ssh_host: String::new(),
            ssh_port: "22".to_string(),
            ssh_username: String::new(),
            ssh_private_key_path: String::new(),
        }
    }

    pub fn mysql_default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: "3306".to_string(),
            username: "root".to_string(),
            ssl_mode: "preferred".to_string(),
            password: String::new(),
            database: String::new(),
            ssh_enabled: false,
            ssh_host: String::new(),
            ssh_port: "22".to_string(),
            ssh_username: String::new(),
            ssh_private_key_path: String::new(),
        }
    }

    pub fn from_postgres_request(request: &ConnectionRequest) -> Self {
        match request {
            ConnectionRequest::Postgres(data) => Self::from_postgres(data),
            _ => Self::postgres_default(),
        }
    }

    pub fn from_clickhouse_request(request: &ConnectionRequest) -> Self {
        match request {
            ConnectionRequest::ClickHouse(data) => Self::from_clickhouse(data),
            _ => Self::clickhouse_default(),
        }
    }

    pub fn from_mysql_request(request: &ConnectionRequest) -> Self {
        match request {
            ConnectionRequest::MySql(data) => Self::from_mysql(data),
            _ => Self::mysql_default(),
        }
    }

    pub fn from_postgres(data: &PostgresFormData) -> Self {
        let (ssh_enabled, ssh_host, ssh_port, ssh_username, ssh_private_key_path) =
            ssh_fields_from_tunnel(&data.ssh_tunnel);
        Self {
            host: data.host.clone(),
            ssl_mode: data.ssl_mode.clone(),
            port: data.port.to_string(),
            username: data.username.clone(),
            password: data.password.clone(),
            database: data.database.clone(),
            ssh_enabled,
            ssh_host,
            ssh_port,
            ssh_username,
            ssh_private_key_path,
        }
    }

    pub fn from_clickhouse(data: &ClickHouseFormData) -> Self {
        let (ssh_enabled, ssh_host, ssh_port, ssh_username, ssh_private_key_path) =
            ssh_fields_from_tunnel(&data.ssh_tunnel);
        Self {
            host: data.host.clone(),
            ssl_mode: String::new(),
            port: data.port.to_string(),
            username: data.username.clone(),
            password: data.password.clone(),
            database: data.database.clone(),
            ssh_enabled,
            ssh_host,
            ssh_port,
            ssh_username,
            ssh_private_key_path,
        }
    }

    pub fn from_mysql(data: &MySqlFormData) -> Self {
        let (ssh_enabled, ssh_host, ssh_port, ssh_username, ssh_private_key_path) =
            ssh_fields_from_tunnel(&data.ssh_tunnel);
        Self {
            host: data.host.clone(),
            ssl_mode: data.ssl_mode.clone(),
            port: data.port.to_string(),
            username: data.username.clone(),
            password: data.password.clone(),
            database: data.database.clone(),
            ssh_enabled,
            ssh_host,
            ssh_port,
            ssh_username,
            ssh_private_key_path,
        }
    }

    pub fn ssh_tunnel(&self) -> Option<SshTunnelConfig> {
        if !self.ssh_enabled {
            return None;
        }

        Some(SshTunnelConfig {
            host: self.ssh_host.clone(),
            port: self.ssh_port.parse().unwrap_or(22),
            username: self.ssh_username.clone(),
            private_key_path: self.ssh_private_key_path.clone(),
        })
    }
}
