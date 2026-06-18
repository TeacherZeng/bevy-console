use bevy::prelude::*;
use clap::Parser;
use std::collections::BTreeMap;

use crate as bevy_console;
use crate::{ConsoleCommand, ConsoleConfiguration, reply};

/// Prints available arguments and usage
#[derive(Parser, ConsoleCommand)]
#[command(name = "help")]
pub(crate) struct HelpCommand {
    /// Help for a given command
    command: Option<String>,
}

/// Lists available commands, optionally filtered by a fuzzy query
#[derive(Parser, ConsoleCommand)]
#[command(name = "list")]
pub(crate) struct ListCommand {
    /// Optional fuzzy query matched against command name and summary
    query: Option<String>,
}

pub(crate) fn help_command(
    mut help: ConsoleCommand<HelpCommand>,
    mut config: ResMut<ConsoleConfiguration>,
) {
    match help.take() {
        Some(Ok(HelpCommand { command: Some(cmd) })) => match config.commands.get_mut(cmd.as_str())
        {
            Some(command_info) => {
                help.reply(command_info.render_long_help().to_string());
            }
            None => {
                let lines = command_list_lines(&config.commands, Some(&cmd));
                if lines.is_empty() {
                    reply!(help, "Command '{}' does not exist", cmd);
                } else {
                    reply!(help, "Matching commands:");
                    for line in lines {
                        help.reply(line);
                    }
                    help.reply("");
                }
            }
        },
        Some(Ok(HelpCommand { command: None })) => {
            debug!("No command received in help");
            reply!(help, "Available commands:");
            for line in command_list_lines(&config.commands, None) {
                help.reply(line);
            }
            help.reply("");
        }
        _ => {}
    }
}

pub(crate) fn list_command(
    mut list: ConsoleCommand<ListCommand>,
    config: Res<ConsoleConfiguration>,
) {
    if let Some(Ok(ListCommand { query })) = list.take() {
        let lines = command_list_lines(&config.commands, query.as_deref());
        let title = if query.is_some() {
            "Matching commands:"
        } else {
            "Available commands:"
        };
        reply!(list, "{title}");
        for line in lines {
            list.reply(line);
        }
        list.reply("");
    }
}

fn command_list_lines(
    commands: &BTreeMap<&'static str, clap::Command>,
    query: Option<&str>,
) -> Vec<String> {
    let query = query.map(str::trim).filter(|query| !query.is_empty());
    let filtered = commands
        .iter()
        .filter(|(name, command)| {
            query.is_none_or(|query| {
                command_matches_query(
                    query,
                    name,
                    command.get_about().map(|about| about.to_string()),
                )
            })
        })
        .collect::<Vec<_>>();
    let longest_command_name = filtered
        .iter()
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or(0);

    filtered
        .into_iter()
        .map(|(name, command)| {
            let mut line = format!("  {name}{}", " ".repeat(longest_command_name - name.len()));
            if let Some(about) = command.get_about() {
                line.push_str(&format!(" - {}", about));
            }
            line.trim_end().to_string()
        })
        .collect()
}

fn command_matches_query(query: &str, name: &str, about: Option<String>) -> bool {
    let query = query.to_lowercase();
    let name = name.to_lowercase();
    if contains_or_fuzzy_matches(&name, &query) {
        return true;
    }

    about
        .map(|about| contains_or_fuzzy_matches(&about.to_lowercase(), &query))
        .unwrap_or(false)
}

fn contains_or_fuzzy_matches(haystack: &str, needle: &str) -> bool {
    if haystack.contains(needle) {
        return true;
    }

    let mut search_from = 0;
    for ch in needle.chars() {
        let Some(offset) = haystack[search_from..].find(ch) else {
            return false;
        };
        search_from += offset + ch.len_utf8();
    }
    true
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn command_list_lines_filters_by_fuzzy_command_name() {
        let mut commands = BTreeMap::new();
        commands.insert("debug.scene.load", clap::Command::new("debug.scene.load"));
        commands.insert("debug.time.set", clap::Command::new("debug.time.set"));
        commands.insert("inventory.give", clap::Command::new("inventory.give"));

        let lines = command_list_lines(&commands, Some("sce"));

        assert_eq!(lines, vec!["  debug.scene.load"]);
    }

    #[test]
    fn command_list_lines_filters_by_about_text() {
        let mut commands = BTreeMap::new();
        commands.insert(
            "debug.scene.load",
            clap::Command::new("debug.scene.load").about("Load a scene by id"),
        );
        commands.insert(
            "debug.time.set",
            clap::Command::new("debug.time.set").about("Set current time"),
        );

        let lines = command_list_lines(&commands, Some("current"));

        assert_eq!(lines, vec!["  debug.time.set - Set current time"]);
    }
}
