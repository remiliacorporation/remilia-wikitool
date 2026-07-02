mod capture;
mod cli;
mod commands;
mod config;
mod encoding;
mod files;
mod grep_scan;
mod json_tools;
mod outline;
mod output;
mod sqlite;
mod text;

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use clap::Parser;

use capture::command_capture;
use cli::{Cli, Command};
use commands::{
    GrepCaps, command_dirs, command_files, command_grep, command_grep_with_matcher, command_slice,
};
use config::load_context_config;
use json_tools::{command_json_find, command_json_select};
use outline::command_outline;
use sqlite::{command_sqlite, command_sqlite_schema};
use text::{TextMatcher, collect_terms, resolve_term_mode};

fn main() -> Result<()> {
    output::mark_command_start();
    let cli = Cli::parse();
    let config = load_context_config(cli.config.as_deref(), cli.no_config)?;
    match &cli.command {
        Command::Files {
            paths,
            path,
            globs,
            extensions,
            with_excluded,
            with_git_ignored,
            skip_nested_repos,
            max,
            max_line_chars,
            max_scan_files,
        } => command_files(
            &cli,
            &config,
            &merged_paths(paths, path),
            globs,
            extensions,
            *with_excluded,
            *with_git_ignored,
            *skip_nested_repos,
            *max,
            *max_line_chars,
            *max_scan_files,
        ),
        Command::Dirs {
            paths,
            path,
            depth,
            with_excluded,
            with_git_ignored,
            skip_nested_repos,
            max,
            max_line_chars,
            max_scan_files,
        } => command_dirs(
            &cli,
            &config,
            &merged_paths(paths, path),
            *depth,
            *with_excluded,
            *with_git_ignored,
            *skip_nested_repos,
            *max,
            *max_line_chars,
            *max_scan_files,
        ),
        Command::Grep {
            args,
            path,
            pattern_file,
            literal,
            ignore_case,
            globs,
            extensions,
            with_excluded,
            with_git_ignored,
            skip_nested_repos,
            max_count_files,
            max_files,
            lines_per_file,
            context,
            max_sample_lines,
            max_line_chars,
            max_scan_files,
            max_file_bytes,
        } => command_grep(
            &cli,
            &config,
            args,
            path,
            pattern_file.as_deref(),
            *literal,
            *ignore_case,
            globs,
            extensions,
            *with_excluded,
            *with_git_ignored,
            *skip_nested_repos,
            &GrepCaps {
                max_count_files: *max_count_files,
                max_files: *max_files,
                lines_per_file: *lines_per_file,
                context: *context,
                max_sample_lines: *max_sample_lines,
                max_line_chars: *max_line_chars,
                max_scan_files: *max_scan_files,
                max_file_bytes: *max_file_bytes,
            },
        ),
        Command::GrepTerms {
            terms,
            term_files,
            mode,
            any,
            all,
            ignore_case,
            globs,
            extensions,
            paths,
            path,
            with_excluded,
            with_git_ignored,
            skip_nested_repos,
            max_count_files,
            max_files,
            lines_per_file,
            context,
            max_sample_lines,
            max_line_chars,
            max_scan_files,
            max_file_bytes,
        } => {
            let terms = collect_terms(terms, term_files)?;
            let mode = resolve_term_mode(*mode, *any, *all)?;
            command_grep_with_matcher(
                &cli,
                &config,
                "grep-terms",
                TextMatcher::terms(terms, mode, *ignore_case),
                &merged_paths(paths, path),
                globs,
                extensions,
                *with_excluded,
                *with_git_ignored,
                *skip_nested_repos,
                &GrepCaps {
                    max_count_files: *max_count_files,
                    max_files: *max_files,
                    lines_per_file: *lines_per_file,
                    context: *context,
                    max_sample_lines: *max_sample_lines,
                    max_line_chars: *max_line_chars,
                    max_scan_files: *max_scan_files,
                    max_file_bytes: *max_file_bytes,
                },
            )
        }
        Command::Slice {
            file,
            path,
            range,
            start,
            end,
            tail,
            lines,
            max_lines,
            max_line_chars,
            char_start,
            chars,
        } => command_slice(
            &cli,
            &config,
            file.as_deref()
                .or(path.as_deref())
                .expect("clap requires a slice file through <FILE> or --path"),
            range.as_deref(),
            *start,
            *end,
            *tail,
            *lines,
            *max_lines,
            *max_line_chars,
            *char_start,
            *chars,
        ),
        Command::Outline {
            file,
            path,
            lang,
            prefix,
            pattern,
            contains,
            ignore_case,
            max_items,
            max_line_chars,
        } => command_outline(
            &cli,
            &config,
            file.as_deref()
                .or(path.as_deref())
                .expect("clap requires an outline file through <FILE> or --path"),
            lang.as_deref(),
            prefix.as_deref(),
            pattern.as_deref(),
            contains,
            *ignore_case,
            *max_items,
            *max_line_chars,
        ),
        Command::JsonFind {
            file,
            path,
            key_contains,
            key_regex,
            path_contains,
            path_regex,
            value_contains,
            max,
            max_value_chars,
        } => command_json_find(
            &cli,
            &config,
            file.as_deref()
                .or(path.as_deref())
                .expect("clap requires a JSON input through <FILE> or --path"),
            key_contains,
            key_regex.as_deref(),
            path_contains,
            path_regex.as_deref(),
            value_contains,
            *max,
            *max_value_chars,
        ),
        Command::JsonSelect {
            file,
            path,
            array,
            fields,
            where_exact,
            where_contains,
            max,
            max_value_chars,
        } => command_json_select(
            &cli,
            &config,
            file.as_deref()
                .or(path.as_deref())
                .expect("clap requires a JSON input through <FILE> or --path"),
            array.as_deref(),
            fields,
            where_exact,
            where_contains,
            *max,
            *max_value_chars,
        ),
        Command::Sqlite {
            positional_db,
            db,
            sql,
            sql_file,
            max_rows,
            max_scan_rows,
            timeout_secs,
            max_value_chars,
        } => {
            let db = resolve_db_path("sqlite", positional_db.as_ref(), db.as_ref())?;
            command_sqlite(
                &cli,
                &config,
                db,
                sql.as_deref(),
                sql_file.as_deref(),
                *max_rows,
                *max_scan_rows,
                *timeout_secs,
                *max_value_chars,
            )
        }
        Command::SqliteSchema {
            positional_db,
            db,
            tables,
            name_contains,
            include_shadow,
            include_system,
            max_tables,
            max_columns,
            max_indexes,
            max_line_chars,
        } => {
            let db = resolve_db_path("sqlite-schema", positional_db.as_ref(), db.as_ref())?;
            command_sqlite_schema(
                &cli,
                &config,
                db,
                tables,
                name_contains,
                *include_shadow,
                *include_system,
                *max_tables,
                *max_columns,
                *max_indexes,
                *max_line_chars,
            )
        }
        Command::Capture {
            max_lines,
            max_bytes,
            max_line_chars,
            argv,
        } => command_capture(&cli, &config, *max_lines, *max_bytes, *max_line_chars, argv),
    }
}

pub(crate) fn merged_paths(positional: &[PathBuf], named: &[PathBuf]) -> Vec<PathBuf> {
    let mut paths = Vec::with_capacity(positional.len() + named.len());
    paths.extend(positional.iter().cloned());
    paths.extend(named.iter().cloned());
    if paths.is_empty() {
        paths.push(PathBuf::from("."));
    }
    paths
}

pub(crate) fn resolve_db_path<'a>(
    command_name: &str,
    positional_db: Option<&'a PathBuf>,
    named_db: Option<&'a PathBuf>,
) -> Result<&'a Path> {
    match (positional_db, named_db) {
        (Some(_), Some(_)) => Err(anyhow!(
            "{command_name} accepts either positional <DB> or --db/--path <DB>, not both"
        )),
        (Some(path), None) | (None, Some(path)) => Ok(path.as_path()),
        (None, None) => Err(anyhow!(
            "{command_name} requires a SQLite database path as <DB> or --db/--path <DB>"
        )),
    }
}

#[cfg(test)]
mod tests;
