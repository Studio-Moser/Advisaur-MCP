mod catalog;
mod config;
mod install;

pub use catalog::{CLIENTS, Client, ClientInfo};
pub use config::{IntegrationError, Result};
pub use install::{
    Change, CommandRunner, IntegrationPaths, IntegrationStatus, setup, status, uninstall,
};
