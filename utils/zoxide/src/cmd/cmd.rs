#![allow(clippy::module_inception)]

use std::path::PathBuf;

pub enum Cmd {
    Add(Add),
    Edit(Edit),
    Query(Query),
    Remove(Remove),
}

pub struct Add {
    pub paths: Vec<PathBuf>,
    pub score: Option<f64>,
}

/// Edit the database
pub struct Edit {
    pub cmd: Option<EditCommand>,
}

pub enum EditCommand {
    Decrement { path: String },
    Delete { path: String },
    Increment { path: String },
    Reload,
}

/// Search for a directory in the database
pub struct Query {
    pub keywords: Vec<String>,

    /// Show unavailable directories
    pub all: bool,

    /// List all matching directories
    pub list: bool,

    /// Print score with results
    pub score: bool,

    /// Exclude the current directory
    pub exclude: Option<String>,

    /// Only search within this directory
    pub base_dir: Option<String>,
}

/// Remove a directory from the database
pub struct Remove {
    pub paths: Vec<String>,
}
