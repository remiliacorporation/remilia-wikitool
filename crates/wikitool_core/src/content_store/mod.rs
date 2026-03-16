use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use reqwest::Url;
use rusqlite::{Connection, params};

use crate::filesystem::{Namespace, ScanStats, ScannedFile};
use crate::knowledge::model::{
    BrokenLinkIssue, DoubleRedirectIssue, LocalContextChunk, LocalMediaUsage, LocalReferenceUsage,
    LocalTemplateInvocation,
};
use crate::runtime::ResolvedPaths;
use crate::schema::open_initialized_database_connection;
use crate::support::table_exists;

pub(crate) mod model;
pub(crate) mod parsing;
pub(crate) mod storage;
