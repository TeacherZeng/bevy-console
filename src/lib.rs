#![doc = include_str ! ("../README.md")]
#![deny(missing_docs)]

use bevy::prelude::*;
pub use bevy_console_derive::ConsoleCommand;
use bevy_egui::{EguiPlugin, EguiPreUpdateSet, EguiPrimaryContextPass};
use console::{ConsoleCache, block_keyboard_input, block_mouse_input};
use trie_rs::TrieBuilder;

#[cfg(feature = "default-commands")]
use crate::commands::{
    clear::{ClearCommand, clear_command},
    exit::{ExitCommand, exit_command},
    help::{HelpCommand, ListCommand, help_command, list_command},
};

pub use crate::console::{
    AddConsoleCommand, Command, ConsoleCommand, ConsoleCommandEntered, ConsoleConfiguration,
    ConsoleOpen, NamedCommand, PrintConsoleLine,
};
pub use crate::log::*;

use crate::console::{ConsoleState, console_ui, receive_console_line};
pub use clap;

mod color;
#[cfg(feature = "default-commands")]
mod commands;
mod console;
mod log;
mod macros;
/// Console plugin.
pub struct ConsolePlugin;

#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
/// The SystemSet for console/command related systems
pub enum ConsoleSet {
    /// Systems configuring commands at startup, only once
    Startup,

    /// Systems operating the console UI (the input layer)
    ConsoleUI,

    /// Systems executing console commands (the functionality layer).
    /// All command handler systems are added to this set
    Commands,

    /// Systems running after command systems, which depend on the fact commands have executed beforehand (the output layer).
    /// For example a system which makes use of [`PrintConsoleLine`] messages should be placed in this set to be able to receive
    /// New lines to print in the same frame
    PostCommands,
}

/// Run condition which does not run any command systems if no command was entered
fn have_commands(commands: MessageReader<ConsoleCommandEntered>) -> bool {
    !commands.is_empty()
}

/// builds the predictive search engine for completions
fn init(config: Res<ConsoleConfiguration>, mut cache: ResMut<ConsoleCache>) {
    debug!("lib.rs:init");
    let mut trie_builder = TrieBuilder::new();
    let mut completion_entries = Vec::new();
    for cmd in config.commands.keys() {
        trie_builder.push(cmd);
        completion_entries.push((*cmd).to_string());
    }

    for completions in &config.arg_completions {
        let completion = completions.join(" ");
        trie_builder.push(&completion);
        completion_entries.push(completion);
    }

    completion_entries.sort();
    completion_entries.dedup();
    cache.completion_entries = completion_entries;
    cache.commands_trie = Some(trie_builder.build());
}

impl Plugin for ConsolePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConsoleConfiguration>()
            .init_resource::<ConsoleState>()
            .init_resource::<ConsoleOpen>()
            .init_resource::<ConsoleCache>()
            .add_message::<ConsoleCommandEntered>()
            .add_message::<PrintConsoleLine>();

        #[cfg(feature = "default-commands")]
        app.add_console_command::<ClearCommand, _>(clear_command)
            .add_console_command::<ExitCommand, _>(exit_command)
            .add_console_command::<HelpCommand, _>(help_command)
            .add_console_command::<ListCommand, _>(list_command);

        // after per-command startup
        app.add_systems(Startup, init.after(ConsoleSet::Startup))
            .add_systems(
                PreUpdate,
                (block_mouse_input, block_keyboard_input)
                    .after(EguiPreUpdateSet::ProcessInput)
                    .before(EguiPreUpdateSet::BeginPass),
            )
            .add_systems(
                EguiPrimaryContextPass,
                (
                    console_ui.in_set(ConsoleSet::ConsoleUI),
                    receive_console_line.in_set(ConsoleSet::PostCommands),
                ),
            )
            .configure_sets(
                EguiPrimaryContextPass,
                (
                    ConsoleSet::Commands
                        .after(ConsoleSet::ConsoleUI)
                        .run_if(have_commands),
                    ConsoleSet::PostCommands.after(ConsoleSet::Commands),
                ),
            );

        // Don't initialize an egui plugin if one already exists.
        // This can happen if another plugin is using egui and was installed before us.
        if !app.is_plugin_added::<EguiPlugin>() {
            app.add_plugins(EguiPlugin::default());
        }
    }
}
