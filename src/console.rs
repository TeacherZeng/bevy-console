use bevy::ecs::query::FilteredAccessSet;
use bevy::ecs::resource::Resource;
use bevy::ecs::{
    change_detection::Tick,
    system::{ScheduleSystem, SystemMeta, SystemParam},
    world::unsafe_world_cell::UnsafeWorldCell,
};
use bevy::platform::hash::FixedState;
use bevy::{input::keyboard::KeyboardInput, prelude::*};
use bevy_egui::egui::{self, TextEdit};
use bevy_egui::egui::{Context, Id};
use bevy_egui::egui::{text::LayoutJob, text_selection::CCursorRange};
use bevy_egui::{
    EguiContexts,
    egui::{Color32, FontId, TextFormat, epaint::text::cursor::CCursor},
};
use clap::{CommandFactory, FromArgMatches};
use core::str;
use shlex::Shlex;
use std::collections::{BTreeMap, VecDeque};
use std::hash::BuildHasher;
use std::marker::PhantomData;
use std::mem;
use trie_rs::Trie;

use crate::{
    ConsoleSet,
    color::{TextFormattingOverride, parse_ansi_styled_str},
};

type ConsoleCommandEnteredReaderSystemParam =
    MessageReader<'static, 'static, ConsoleCommandEntered>;

type PrintConsoleLineWriterSystemParam = MessageWriter<'static, PrintConsoleLine>;

/// A super-trait for command like structures
pub trait Command: NamedCommand + CommandFactory + FromArgMatches + Sized + Resource {}
impl<T: NamedCommand + CommandFactory + FromArgMatches + Sized + Resource> Command for T {}

/// Trait used to allow uniquely identifying commands at compile time
pub trait NamedCommand {
    /// Return the unique command identifier (same as the command "executable")
    fn name() -> &'static str;
}

/// Executed parsed console command.
///
/// Used to capture console commands which implement [`CommandName`], [`CommandArgs`] & [`CommandHelp`].
/// These can be easily implemented with the [`ConsoleCommand`](bevy_console_derive::ConsoleCommand) derive macro.
///
/// # Example
///
/// ```
/// # use bevy_console::ConsoleCommand;
/// # use clap::Parser;
/// /// Prints given arguments to the console.
/// #[derive(Parser, ConsoleCommand)]
/// #[command(name = "log")]
/// struct LogCommand {
///     /// Message to print
///     msg: String,
///     /// Number of times to print message
///     num: Option<i64>,
/// }
///
/// fn log_command(mut log: ConsoleCommand<LogCommand>) {
///     if let Some(Ok(LogCommand { msg, num })) = log.take() {
///         log.ok();
///     }
/// }
/// ```
pub struct ConsoleCommand<'w, T> {
    command: Option<Result<T, clap::Error>>,
    console_line: MessageWriter<'w, PrintConsoleLine>,
}

impl<T> ConsoleCommand<'_, T> {
    ///
    /// Returns Some(T) if the command was executed and arguments were valid.
    ///
    /// This method should only be called once.
    /// Consecutive calls will return None regardless if the command occurred.
    pub fn take(&mut self) -> Option<Result<T, clap::Error>> {
        mem::take(&mut self.command)
    }

    /// Print `[ok]` in the console.
    pub fn ok(&mut self) {
        self.console_line
            .write(PrintConsoleLine::new("[ok]".into()));
    }

    /// Print `[failed]` in the console.
    pub fn failed(&mut self) {
        self.console_line
            .write(PrintConsoleLine::new("[failed]".into()));
    }

    /// Print a reply in the console.
    ///
    /// See [`reply!`](crate::reply) for usage with the [`format!`] syntax.
    pub fn reply(&mut self, msg: impl Into<String>) {
        self.console_line.write(PrintConsoleLine::new(msg.into()));
    }

    /// Print a reply in the console followed by `[ok]`.
    ///
    /// See [`reply_ok!`](crate::reply_ok) for usage with the [`format!`] syntax.
    pub fn reply_ok(&mut self, msg: impl Into<String>) {
        self.console_line.write(PrintConsoleLine::new(msg.into()));
        self.ok();
    }

    /// Print a reply in the console followed by `[failed]`.
    ///
    /// See [`reply_failed!`](crate::reply_failed) for usage with the [`format!`] syntax.
    pub fn reply_failed(&mut self, msg: impl Into<String>) {
        self.console_line.write(PrintConsoleLine::new(msg.into()));
        self.failed();
    }
}

pub struct ConsoleCommandState<T> {
    #[allow(clippy::type_complexity)]
    message_reader: <ConsoleCommandEnteredReaderSystemParam as SystemParam>::State,
    console_line: <PrintConsoleLineWriterSystemParam as SystemParam>::State,
    marker: PhantomData<T>,
}

unsafe impl<T: Command> SystemParam for ConsoleCommand<'_, T> {
    type State = ConsoleCommandState<T>;
    type Item<'w, 's> = ConsoleCommand<'w, T>;

    fn init_state(world: &mut World) -> Self::State {
        let message_reader = ConsoleCommandEnteredReaderSystemParam::init_state(world);
        let console_line = PrintConsoleLineWriterSystemParam::init_state(world);
        ConsoleCommandState {
            message_reader,
            console_line,
            marker: PhantomData,
        }
    }

    fn init_access(
        _state: &Self::State,
        _system_meta: &mut SystemMeta,
        _component_access_set: &mut FilteredAccessSet,
        _world: &mut World,
    ) {
    }

    #[inline]
    unsafe fn get_param<'w, 's>(
        state: &'s mut Self::State,
        system_meta: &SystemMeta,
        world: UnsafeWorldCell<'w>,
        change_tick: Tick,
    ) -> Self::Item<'w, 's> {
        unsafe {
            let mut message_reader = ConsoleCommandEnteredReaderSystemParam::get_param(
                &mut state.message_reader,
                system_meta,
                world,
                change_tick,
            );
            let mut console_line = PrintConsoleLineWriterSystemParam::get_param(
                &mut state.console_line,
                system_meta,
                world,
                change_tick,
            );

            let command = message_reader.read().find_map(|command| {
                if T::name() == command.command_name {
                    let clap_command = T::command().no_binary_name(true);
                    // .color(clap::ColorChoice::Always);
                    let arg_matches = clap_command.try_get_matches_from(command.args.iter());

                    debug!(
                        "Trying to parse as `{}`. Result: {arg_matches:?}",
                        command.command_name
                    );

                    match arg_matches {
                        Ok(matches) => {
                            return Some(T::from_arg_matches(&matches));
                        }
                        Err(err) => {
                            console_line.write(PrintConsoleLine::new(err.to_string()));
                            return Some(Err(err));
                        }
                    }
                }
                None
            });

            ConsoleCommand {
                command,
                console_line,
            }
        }
    }
}
/// Parsed raw console command into `command` and `args`.
#[derive(Clone, Debug, Event, Message)]
pub struct ConsoleCommandEntered {
    /// the command definition
    pub command_name: String,
    /// Raw parsed arguments
    pub args: Vec<String>,
}

/// Events to print to the console.
#[derive(Clone, Debug, Eq, Event, PartialEq, Message)]
pub struct PrintConsoleLine {
    /// Console line
    pub line: String,
}

impl PrintConsoleLine {
    /// Creates a new console line to print.
    pub const fn new(line: String) -> Self {
        Self { line }
    }
}

/// Console configuration
#[derive(Resource)]
pub struct ConsoleConfiguration {
    /// Registered keys for toggling the console
    pub keys: Vec<KeyCode>,
    /// Left position
    pub left_pos: f32,
    /// Top position
    pub top_pos: f32,
    /// Console height
    pub height: f32,
    /// Console width
    pub width: f32,
    /// Registered console commands
    pub commands: BTreeMap<&'static str, clap::Command>,
    /// Number of commands to store in history
    pub history_size: usize,
    /// Line prefix symbol
    pub symbol: String,
    /// allows window to be collpased
    pub collapsible: bool,
    /// Title name of console window
    pub title_name: String,
    /// allows window to be resizable
    pub resizable: bool,
    /// allows window to be movable
    pub moveable: bool,
    /// show the title bar or not
    pub show_title_bar: bool,
    /// Background color of console window
    pub background_color: Color32,
    /// Foreground (text) color
    pub foreground_color: Color32,
    /// Number of suggested commands to show
    pub num_suggestions: usize,
    /// Background color of the suggestions popup
    pub suggestion_background_color: Color32,
    /// Border color of the suggestions popup
    pub suggestion_border_color: Color32,
    /// Background color of the selected suggestion row
    pub suggestion_selected_background_color: Color32,
    /// Blocks mouse from clicking through console
    pub block_mouse: bool,
    /// Blocks keyboard from interacting outside console when active
    pub block_keyboard: bool,
    /// Custom completion sequences,
    /// for example [vec!["custom", "foo"]], will complete `custom foo` when typing `custom`
    pub arg_completions: Vec<Vec<String>>,
}

#[derive(Resource, Default)]
pub struct ConsoleCache {
    /// Trie used for completions, autogenerated from registered console commands
    /// this probably should operate over references to save memory, but this is convenient for now
    pub(crate) commands_trie: Option<Trie<u8>>,
    pub(crate) completion_entries: Vec<String>,
    pub(crate) predictions_hash_key: Option<u64>,
    pub(crate) predictions_cache: Vec<String>,
    pub(crate) prediction_matches_buffer: bool,
}

impl Default for ConsoleConfiguration {
    fn default() -> Self {
        Self {
            keys: vec![KeyCode::Backquote],
            left_pos: 200.0,
            top_pos: 100.0,
            height: 400.0,
            width: 800.0,
            commands: BTreeMap::new(),
            history_size: 20,
            symbol: "$ ".to_owned(),
            collapsible: false,
            title_name: "Console".to_string(),
            resizable: true,
            moveable: true,
            show_title_bar: true,
            background_color: Color32::from_black_alpha(102),
            foreground_color: Color32::LIGHT_GRAY,
            num_suggestions: 4,
            suggestion_background_color: Color32::from_black_alpha(230),
            suggestion_border_color: Color32::from_gray(96),
            suggestion_selected_background_color: Color32::from_rgb(48, 92, 150),
            block_mouse: false,
            block_keyboard: false,
            arg_completions: Default::default(),
        }
    }
}

impl Clone for ConsoleConfiguration {
    fn clone(&self) -> ConsoleConfiguration {
        ConsoleConfiguration {
            keys: self.keys.clone(),
            left_pos: self.left_pos,
            top_pos: self.top_pos,
            height: self.height,
            width: self.width,
            commands: self.commands.clone(),
            history_size: self.history_size,
            symbol: self.symbol.clone(),
            arg_completions: self.arg_completions.clone(),
            collapsible: self.collapsible,
            title_name: self.title_name.clone(),
            resizable: self.resizable,
            moveable: self.moveable,
            show_title_bar: self.show_title_bar,
            background_color: self.background_color,
            foreground_color: self.foreground_color,
            num_suggestions: self.num_suggestions,
            suggestion_background_color: self.suggestion_background_color,
            suggestion_border_color: self.suggestion_border_color,
            suggestion_selected_background_color: self.suggestion_selected_background_color,
            block_mouse: self.block_mouse,
            block_keyboard: self.block_keyboard,
        }
    }
}

/// Add a console commands to Bevy app.
pub trait AddConsoleCommand {
    /// Add a console command with a given system.
    ///
    /// This registers the console command so it will print with the built-in `help` console command.
    ///
    /// # Example
    ///
    /// ```
    /// # use bevy::prelude::*;
    /// # use bevy_console::{AddConsoleCommand, ConsoleCommand};
    /// # use clap::Parser;
    /// App::new()
    ///     .add_console_command::<LogCommand, _>(log_command);
    /// #
    /// # /// Prints given arguments to the console.
    /// # #[derive(Parser, ConsoleCommand)]
    /// # #[command(name = "log")]
    /// # struct LogCommand;
    /// #
    /// # fn log_command(mut log: ConsoleCommand<LogCommand>) {}
    /// ```
    fn add_console_command<T: Command, Params>(
        &mut self,
        system: impl IntoScheduleConfigs<ScheduleSystem, Params>,
    ) -> &mut Self;
}

impl AddConsoleCommand for App {
    fn add_console_command<T: Command, Params>(
        &mut self,
        system: impl IntoScheduleConfigs<ScheduleSystem, Params>,
    ) -> &mut Self {
        let sys = move |mut config: ResMut<ConsoleConfiguration>| {
            let command = T::command().no_binary_name(true);
            // .color(clap::ColorChoice::Always);
            let name = T::name();
            if config.commands.contains_key(name) {
                warn!(
                    "console command '{}' already registered and was overwritten",
                    name
                );
            }
            config.commands.insert(name, command);
        };

        self.add_systems(Startup, sys.in_set(ConsoleSet::Startup))
            .add_systems(Update, system.in_set(ConsoleSet::Commands))
    }
}

/// Console open state
#[derive(Default, Resource)]
pub struct ConsoleOpen {
    /// Console open
    pub open: bool,
}

#[derive(Resource)]
pub(crate) struct ConsoleState {
    pub(crate) buf: String,
    pub(crate) scrollback: Vec<String>,
    pub(crate) history: VecDeque<String>,
    pub(crate) history_index: usize,
    pub(crate) suggestion_index: Option<usize>,
}

impl Default for ConsoleState {
    fn default() -> Self {
        ConsoleState {
            buf: String::default(),
            scrollback: Vec::new(),
            history: VecDeque::from([String::new()]),
            history_index: 0,
            suggestion_index: None,
        }
    }
}

fn default_style(config: &ConsoleConfiguration) -> TextFormat {
    TextFormat::simple(FontId::monospace(14f32), config.foreground_color)
}

fn style_ansi_text(str: &str, config: &ConsoleConfiguration) -> LayoutJob {
    let mut layout_job = LayoutJob::default();
    for (str, overrides) in parse_ansi_styled_str(str).into_iter() {
        let mut current_style = default_style(config);

        for o in overrides {
            match o {
                TextFormattingOverride::Bold => current_style.font_id.size = 16f32, // no support for bold font families in egui TODO: when egui supports bold font families, use them here
                TextFormattingOverride::Dim => {
                    // no support for dim font families in egui TODO: when egui supports dim font families, use them here
                    current_style.color = current_style.color.gamma_multiply(0.5);
                }
                TextFormattingOverride::Italic => current_style.italics = true,
                TextFormattingOverride::Underline => {
                    current_style.underline = egui::Stroke::new(1., config.foreground_color)
                }
                TextFormattingOverride::Strikethrough => {
                    current_style.strikethrough = egui::Stroke::new(1., config.foreground_color)
                }
                TextFormattingOverride::Foreground(c) => current_style.color = c,
                TextFormattingOverride::Background(c) => current_style.background = c,
                _ => {}
            }
        }

        if !str.is_empty() {
            layout_job.append(str, 0f32, current_style.clone());
        }
    }
    layout_job
}

fn completion_candidates(query: &str, entries: &[String], suggestion_count: usize) -> Vec<String> {
    let query = query.trim();
    if query.is_empty() || suggestion_count == 0 {
        return Vec::new();
    }

    let query_lower = query.to_lowercase();
    let mut ranked = entries
        .iter()
        .filter_map(|entry| {
            completion_rank(&query_lower, &entry.to_lowercase())
                .map(|(bucket, position)| (bucket, position, entry.len(), entry.as_str()))
        })
        .collect::<Vec<_>>();

    ranked.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
            .then_with(|| a.3.cmp(b.3))
    });

    ranked
        .into_iter()
        .map(|(_, _, _, entry)| entry.to_string())
        .take(suggestion_count)
        .collect()
}

fn completion_rank(query_lower: &str, candidate_lower: &str) -> Option<(u8, usize)> {
    if candidate_lower.starts_with(query_lower) {
        return Some((0, 0));
    }

    if let Some(position) = candidate_lower.find(query_lower) {
        return Some((1, position));
    }

    fuzzy_match_position(query_lower, candidate_lower).map(|position| (2, position))
}

fn fuzzy_match_position(query_lower: &str, candidate_lower: &str) -> Option<usize> {
    let mut first_match = None;
    let mut search_from = 0;

    for needle in query_lower.chars() {
        let haystack = &candidate_lower[search_from..];
        let Some(offset) = haystack.find(needle) else {
            return None;
        };
        let position = search_from + offset;
        first_match.get_or_insert(position);
        search_from = position + needle.len_utf8();
    }

    first_match
}

fn clear_prediction_popup(state: &mut ConsoleState, cache: &mut ConsoleCache) {
    cache.predictions_cache.clear();
    cache.predictions_hash_key = None;
    cache.prediction_matches_buffer = false;
    state.suggestion_index = None;
}

fn accept_selected_suggestion(state: &mut ConsoleState, cache: &mut ConsoleCache) -> bool {
    if cache.predictions_cache.is_empty() || cache.prediction_matches_buffer {
        state.suggestion_index = None;
        return false;
    }

    let index = state
        .suggestion_index
        .unwrap_or(0)
        .min(cache.predictions_cache.len() - 1);
    state.buf = cache.predictions_cache[index].clone();
    clear_prediction_popup(state, cache);
    true
}

fn move_suggestion_selection(
    state: &mut ConsoleState,
    cache: &ConsoleCache,
    direction: isize,
) -> bool {
    if cache.predictions_cache.is_empty() || cache.prediction_matches_buffer {
        state.suggestion_index = None;
        return false;
    }

    let len = cache.predictions_cache.len();
    let current = state.suggestion_index.unwrap_or(0) as isize;
    let next = (current + direction).rem_euclid(len as isize) as usize;
    state.suggestion_index = Some(next);
    true
}

fn handle_tab_completion(state: &mut ConsoleState, cache: &mut ConsoleCache) -> bool {
    accept_selected_suggestion(state, cache)
}

fn should_show_suggestions_popup(
    has_focus: bool,
    state: &ConsoleState,
    cache: &ConsoleCache,
) -> bool {
    has_focus
        && !state.buf.is_empty()
        && !cache.prediction_matches_buffer
        && !cache.predictions_cache.is_empty()
}

/// Recompute predictions for the console based on the current buffer content.
/// if the buffer does not change the predictions are not recomputed.
pub(crate) fn recompute_predictions(
    state: &mut ConsoleState,
    cache: &mut ConsoleCache,
    suggestion_count: usize,
) {
    if state.buf.is_empty() {
        cache.predictions_cache.clear();
        cache.predictions_hash_key = None;
        cache.prediction_matches_buffer = false;
        state.suggestion_index = None;
        return;
    }

    let hash = FixedState::with_seed(42).hash_one(&state.buf);

    let recompute = if let Some(predictions_hash_key) = cache.predictions_hash_key {
        predictions_hash_key != hash
    } else {
        true
    };

    if recompute {
        let words = Shlex::new(&state.buf).collect::<Vec<_>>();
        let query = words.join(" ");

        cache.predictions_cache =
            completion_candidates(&query, &cache.completion_entries, suggestion_count);

        cache.predictions_hash_key = Some(hash);
        cache.prediction_matches_buffer = false;
        state.suggestion_index = None;

        if cache
            .predictions_cache
            .iter()
            .any(|candidate| candidate == &state.buf)
        {
            cache.prediction_matches_buffer = true;
        } else if !cache.predictions_cache.is_empty() {
            state.suggestion_index = Some(0);
        }
    }
}

pub(crate) fn console_ui(
    mut egui_context: EguiContexts,
    config: Res<ConsoleConfiguration>,
    mut cache: ResMut<ConsoleCache>,
    mut keyboard_input_events: MessageReader<KeyboardInput>,
    mut state: ResMut<ConsoleState>,
    command_entered: MessageWriter<ConsoleCommandEntered>,
    mut console_open: ResMut<ConsoleOpen>,
) {
    let keyboard_input_events = keyboard_input_events.read().collect::<Vec<_>>();

    // If there is no egui context, return (can happen when exiting the app)
    let ctx = if let Ok(ctxt) = egui_context.ctx_mut() {
        ctxt
    } else {
        return;
    };

    let pressed = keyboard_input_events
        .iter()
        .any(|code| console_key_pressed(code, &config.keys));

    let mut open_status_changed = false;

    // Toggle console
    if pressed && (console_open.open || !ctx.wants_keyboard_input()) {
        console_open.open = !console_open.open;
        open_status_changed = true;
    }

    if !console_open.open {
        return;
    }

    // Recompute predictions if the buffer changed
    recompute_predictions(&mut state, &mut cache, config.num_suggestions);

    egui::Window::new(&config.title_name)
        .collapsible(config.collapsible)
        .default_pos([config.left_pos, config.top_pos])
        .default_size([config.width, config.height])
        .resizable(config.resizable)
        .movable(config.moveable)
        .title_bar(config.show_title_bar)
        .frame(egui::Frame {
            fill: config.background_color,
            ..Default::default()
        })
        .show(ctx, |ui| {
            ui.style_mut().visuals.extreme_bg_color = config.background_color;
            ui.style_mut().visuals.override_text_color = Some(config.foreground_color);

            // ------------------------
            // Bottom panel: input area
            // ------------------------
            egui::TopBottomPanel::bottom("console_input_panel")
                .exact_height(36.0)
                .show_inside(ui, |ui| {
                    ui.separator();

                    // Ctrl+C clears input
                    if ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::C)) {
                        state.buf.clear();
                        return;
                    }

                    // Ctrl+L clears history
                    if ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::L)) {
                        state.scrollback.clear();
                        return;
                    }

                    let text_edit = egui::TextEdit::singleline(&mut state.buf)
                        .desired_width(f32::INFINITY)
                        .lock_focus(true)
                        .font(egui::TextStyle::Monospace);

                    let text_edit_response = ui.add(text_edit);

                    // Handle enter
                    handle_enter(
                        &config,
                        &mut cache,
                        &mut state,
                        command_entered,
                        ui,
                        &text_edit_response,
                    );

                    let suggestions_popup_visible = should_show_suggestions_popup(
                        text_edit_response.has_focus(),
                        &state,
                        &cache,
                    );

                    // Suggestion and history navigation
                    if text_edit_response.has_focus()
                        && suggestions_popup_visible
                        && ui.input(|i| i.key_pressed(egui::Key::ArrowDown))
                    {
                        move_suggestion_selection(&mut state, &cache, 1);
                    } else if text_edit_response.has_focus()
                        && suggestions_popup_visible
                        && ui.input(|i| i.key_pressed(egui::Key::ArrowUp))
                    {
                        move_suggestion_selection(&mut state, &cache, -1);
                    } else if text_edit_response.has_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::ArrowUp))
                        && state.history.len() > 1
                        && state.history_index < state.history.len() - 1
                    {
                        if state.history_index == 0 && !state.buf.trim().is_empty() {
                            *state.history.get_mut(0).unwrap() = state.buf.clone();
                        }

                        state.history_index += 1;
                        state.buf = state.history[state.history_index].clone();
                        set_cursor_pos(ui.ctx(), text_edit_response.id, state.buf.len());
                    } else if text_edit_response.has_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::ArrowDown))
                        && state.history_index > 0
                    {
                        state.history_index -= 1;
                        state.buf = state.history[state.history_index].clone();
                        set_cursor_pos(ui.ctx(), text_edit_response.id, state.buf.len());
                    }

                    // Shift+Tab navigates suggestions, Tab accepts the highlighted suggestion
                    if text_edit_response.has_focus()
                        && suggestions_popup_visible
                        && ui.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::Tab))
                    {
                        move_suggestion_selection(&mut state, &cache, -1);
                    } else if text_edit_response.has_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Tab))
                        && handle_tab_completion(&mut state, &mut cache)
                    {
                        ui.memory_mut(|m| m.request_focus(text_edit_response.id));
                        set_cursor_pos(ui.ctx(), text_edit_response.id, state.buf.len());
                    }

                    // Focus input when console just opened
                    if open_status_changed {
                        ui.memory_mut(|m| m.request_focus(text_edit_response.id));
                    }

                    // Suggestions popup
                    if should_show_suggestions_popup(text_edit_response.has_focus(), &state, &cache)
                    {
                        let suggestions_area = egui::Area::new(ui.auto_id_with("suggestions"))
                            .fixed_pos(text_edit_response.rect.left_bottom())
                            .movable(false);

                        suggestions_area.show(ui.ctx(), |ui| {
                            egui::Frame::default()
                                .fill(config.suggestion_background_color)
                                .stroke(egui::Stroke::new(1.0, config.suggestion_border_color))
                                .inner_margin(egui::Margin::same(6))
                                .show(ui, |ui| {
                                    ui.set_min_width(config.width);

                                    for (i, suggestion) in
                                        cache.predictions_cache.iter().enumerate()
                                    {
                                        let is_highlighted = Some(i) == state.suggestion_index;

                                        let mut layout_job = egui::text::LayoutJob::default();
                                        let mut style = egui::TextFormat {
                                            font_id: egui::FontId::new(
                                                14.0,
                                                egui::FontFamily::Monospace,
                                            ),
                                            color: egui::Color32::WHITE,
                                            ..Default::default()
                                        };

                                        if is_highlighted {
                                            style.background =
                                                config.suggestion_selected_background_color;
                                        }

                                        layout_job.append(suggestion, 0.0, style);
                                        ui.label(layout_job);
                                    }
                                });
                        });
                    }
                });

            // ------------------------
            // Central panel: scrollback
            // ------------------------
            egui::CentralPanel::default().show_inside(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in &state.scrollback {
                            ui.label(style_ansi_text(line, &config));
                        }

                        // Scroll to bottom if console just opened
                        if console_open.is_changed() {
                            ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                        }
                    });
            });
        });
}

fn handle_enter(
    config: &Res<'_, ConsoleConfiguration>,
    cache: &mut ResMut<'_, ConsoleCache>,
    state: &mut ResMut<'_, ConsoleState>,
    mut command_entered: MessageWriter<'_, ConsoleCommandEntered>,
    ui: &mut egui::Ui,
    text_edit_response: &egui::Response,
) {
    // Handle enter
    if text_edit_response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        if accept_selected_suggestion(state, cache) {
            ui.memory_mut(|m| m.request_focus(text_edit_response.id));
            set_cursor_pos(ui.ctx(), text_edit_response.id, state.buf.len());
            return;
        }

        if state.buf.trim().is_empty() {
            state.scrollback.push(String::new());
        } else {
            let msg = format!("{}{}", config.symbol, state.buf);
            state.scrollback.push(msg);
            let cmd_string = state.buf.clone();
            state.history.insert(1, cmd_string);
            if state.history.len() > config.history_size + 1 {
                state.history.pop_back();
            }
            state.history_index = 0;

            let mut args = Shlex::new(&state.buf).collect::<Vec<_>>();

            if !args.is_empty() {
                let command_name = args.remove(0);
                debug!("Command entered: `{command_name}`, with args: `{args:?}`");

                let command = config.commands.get(command_name.as_str());

                if command.is_some() {
                    command_entered.write(ConsoleCommandEntered { command_name, args });
                } else {
                    debug!(
                        "Command not recognized, recognized commands: `{:?}`",
                        config.commands.keys().collect::<Vec<_>>()
                    );

                    state.scrollback.push("error: Invalid command".into());
                }
            }

            state.buf.clear();
        }

        clear_prediction_popup(state, cache);
        ui.memory_mut(|m| m.request_focus(text_edit_response.id));
        set_cursor_pos(ui.ctx(), text_edit_response.id, state.buf.len());
    }
}

pub(crate) fn receive_console_line(
    mut console_state: ResMut<ConsoleState>,
    mut messages: MessageReader<PrintConsoleLine>,
) {
    for message in messages.read() {
        let message: &PrintConsoleLine = message;
        console_state.scrollback.push(message.line.clone());
    }
}

fn console_key_pressed(keyboard_input: &KeyboardInput, configured_keys: &[KeyCode]) -> bool {
    if !keyboard_input.state.is_pressed() {
        return false;
    }

    for configured_key in configured_keys {
        if configured_key == &keyboard_input.key_code {
            return true;
        }
    }

    false
}

fn set_cursor_pos(ctx: &Context, id: Id, pos: usize) {
    if let Some(mut state) = TextEdit::load_state(ctx, id) {
        state
            .cursor
            .set_char_range(Some(CCursorRange::one(CCursor::new(pos))));
        state.store(ctx, id);
    }
}

pub fn block_mouse_input(
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    config: Res<ConsoleConfiguration>,
    mut contexts: EguiContexts,
) {
    if !config.block_mouse {
        return;
    }

    let Ok(context) = contexts.ctx_mut() else {
        return;
    };

    if context.is_pointer_over_area() || context.wants_pointer_input() {
        mouse.reset_all();
    }
}

pub fn block_keyboard_input(
    mut keyboard_keycode: ResMut<ButtonInput<KeyCode>>,
    config: Res<ConsoleConfiguration>,
    mut contexts: EguiContexts,
) {
    if !config.block_keyboard {
        return;
    }

    let Ok(context) = contexts.ctx_mut() else {
        return;
    };

    if context.wants_keyboard_input() {
        keyboard_keycode.reset_all();
    }
}

#[cfg(test)]
mod tests {
    use bevy::input::ButtonState;
    use bevy::input::keyboard::{Key, NativeKey, NativeKeyCode};

    use super::*;

    #[test]
    fn test_console_key_pressed_scan_code() {
        let input = KeyboardInput {
            key_code: KeyCode::Unidentified(NativeKeyCode::Xkb(41)),
            logical_key: Key::Unidentified(NativeKey::Xkb(41)),
            state: ButtonState::Pressed,
            window: Entity::PLACEHOLDER,
            repeat: false,
            text: None,
        };

        let config = vec![KeyCode::Unidentified(NativeKeyCode::Xkb(41))];

        let result = console_key_pressed(&input, &config);
        assert!(result);
    }

    #[test]
    fn test_console_wrong_key_pressed_scan_code() {
        let input = KeyboardInput {
            key_code: KeyCode::Unidentified(NativeKeyCode::Xkb(42)),
            logical_key: Key::Unidentified(NativeKey::Xkb(42)),
            state: ButtonState::Pressed,
            window: Entity::PLACEHOLDER,
            repeat: false,
            text: None,
        };

        let config = vec![KeyCode::Unidentified(NativeKeyCode::Xkb(41))];

        let result = console_key_pressed(&input, &config);
        assert!(!result);
    }

    #[test]
    fn test_console_key_pressed_key_code() {
        let input = KeyboardInput {
            key_code: KeyCode::Backquote,
            logical_key: Key::Character("`".into()),
            state: ButtonState::Pressed,
            window: Entity::PLACEHOLDER,
            repeat: false,
            text: None,
        };

        let config = vec![KeyCode::Backquote];

        let result = console_key_pressed(&input, &config);
        assert!(result);
    }

    #[test]
    fn test_console_wrong_key_pressed_key_code() {
        let input = KeyboardInput {
            key_code: KeyCode::KeyA,
            logical_key: Key::Character("A".into()),
            state: ButtonState::Pressed,
            window: Entity::PLACEHOLDER,
            repeat: false,
            text: None,
        };

        let config = vec![KeyCode::Backquote];

        let result = console_key_pressed(&input, &config);
        assert!(!result);
    }

    #[test]
    fn test_console_key_right_key_but_not_pressed() {
        let input = KeyboardInput {
            key_code: KeyCode::Backquote,
            logical_key: Key::Character("`".into()),
            state: ButtonState::Released,
            window: Entity::PLACEHOLDER,
            repeat: false,
            text: None,
        };

        let config = vec![KeyCode::Backquote];

        let result = console_key_pressed(&input, &config);
        assert!(!result);
    }

    #[test]
    fn completion_candidates_rank_prefix_before_substring_and_fuzzy() {
        let entries = vec![
            "debug.scene.load".to_string(),
            "debug.time.set".to_string(),
            "scene.inspect".to_string(),
            "spawn.crop.enable".to_string(),
        ];

        let result = completion_candidates("sce", &entries, 4);

        assert_eq!(
            result,
            vec![
                "scene.inspect".to_string(),
                "debug.scene.load".to_string(),
                "spawn.crop.enable".to_string(),
            ]
        );
    }

    #[test]
    fn tab_completion_accepts_single_candidate() {
        let mut state = ConsoleState {
            buf: "debug.sc".to_string(),
            ..Default::default()
        };
        let mut cache = ConsoleCache {
            predictions_cache: vec!["debug.scene.load".to_string()],
            prediction_matches_buffer: false,
            ..Default::default()
        };

        let accepted = handle_tab_completion(&mut state, &mut cache);

        assert!(accepted);
        assert_eq!(state.buf, "debug.scene.load");
        assert_eq!(state.suggestion_index, None);
        assert!(cache.predictions_cache.is_empty());
    }

    #[test]
    fn tab_completion_accepts_top_candidate_when_multiple_match() {
        let mut state = ConsoleState {
            buf: "scene".to_string(),
            ..Default::default()
        };
        let mut cache = ConsoleCache {
            predictions_cache: vec!["scene.inspect".to_string(), "debug.scene.load".to_string()],
            prediction_matches_buffer: false,
            ..Default::default()
        };

        assert!(handle_tab_completion(&mut state, &mut cache));
        assert_eq!(state.buf, "scene.inspect");
        assert_eq!(state.suggestion_index, None);
        assert!(cache.predictions_cache.is_empty());
    }

    #[test]
    fn tab_completion_accepts_currently_selected_candidate() {
        let mut state = ConsoleState {
            buf: "scene".to_string(),
            suggestion_index: Some(1),
            ..Default::default()
        };
        let mut cache = ConsoleCache {
            predictions_cache: vec!["scene.inspect".to_string(), "debug.scene.load".to_string()],
            prediction_matches_buffer: false,
            ..Default::default()
        };

        assert!(handle_tab_completion(&mut state, &mut cache));
        assert_eq!(state.buf, "debug.scene.load");
        assert_eq!(state.suggestion_index, None);
        assert!(cache.predictions_cache.is_empty());
    }

    #[test]
    fn selected_suggestion_can_be_accepted_before_submit() {
        let mut state = ConsoleState {
            buf: "scene".to_string(),
            suggestion_index: Some(1),
            ..Default::default()
        };
        let mut cache = ConsoleCache {
            predictions_cache: vec!["scene.inspect".to_string(), "debug.scene.load".to_string()],
            prediction_matches_buffer: false,
            ..Default::default()
        };

        let accepted = accept_selected_suggestion(&mut state, &mut cache);

        assert!(accepted);
        assert_eq!(state.buf, "debug.scene.load");
        assert_eq!(state.suggestion_index, None);
        assert!(cache.predictions_cache.is_empty());
    }

    #[test]
    fn recompute_predictions_selects_top_candidate_by_default() {
        let mut state = ConsoleState {
            buf: "scene".to_string(),
            ..Default::default()
        };
        let mut cache = ConsoleCache {
            completion_entries: vec!["scene.inspect".to_string(), "debug.scene.load".to_string()],
            ..Default::default()
        };

        recompute_predictions(&mut state, &mut cache, 8);

        assert_eq!(
            cache.predictions_cache,
            vec!["scene.inspect".to_string(), "debug.scene.load".to_string()]
        );
        assert!(!cache.prediction_matches_buffer);
        assert_eq!(state.suggestion_index, Some(0));
    }

    #[test]
    fn recompute_predictions_does_not_select_exact_command_match() {
        let mut state = ConsoleState {
            buf: "scene.inspect".to_string(),
            ..Default::default()
        };
        let mut cache = ConsoleCache {
            completion_entries: vec!["scene.inspect".to_string()],
            ..Default::default()
        };

        recompute_predictions(&mut state, &mut cache, 8);

        assert!(cache.prediction_matches_buffer);
        assert_eq!(state.suggestion_index, None);
    }

    #[test]
    fn recompute_predictions_hides_popup_for_exact_alias_with_longer_canonical_match() {
        let mut state = ConsoleState {
            buf: "grid".to_string(),
            ..Default::default()
        };
        let mut cache = ConsoleCache {
            completion_entries: vec!["grid".to_string(), "debug.grid".to_string()],
            ..Default::default()
        };

        recompute_predictions(&mut state, &mut cache, 8);

        assert_eq!(
            cache.predictions_cache,
            vec!["grid".to_string(), "debug.grid".to_string()]
        );
        assert!(cache.prediction_matches_buffer);
        assert_eq!(state.suggestion_index, None);
        assert!(!should_show_suggestions_popup(true, &state, &cache));
        assert!(!accept_selected_suggestion(&mut state, &mut cache));
    }

    #[test]
    fn suggestion_selection_moves_with_wraparound() {
        let mut state = ConsoleState {
            buf: "scene".to_string(),
            suggestion_index: Some(0),
            ..Default::default()
        };
        let cache = ConsoleCache {
            predictions_cache: vec![
                "scene.inspect".to_string(),
                "scene.load".to_string(),
                "scene.reload".to_string(),
            ],
            prediction_matches_buffer: false,
            ..Default::default()
        };

        assert!(move_suggestion_selection(&mut state, &cache, 1));
        assert_eq!(state.suggestion_index, Some(1));

        assert!(move_suggestion_selection(&mut state, &cache, -1));
        assert_eq!(state.suggestion_index, Some(0));

        assert!(move_suggestion_selection(&mut state, &cache, -1));
        assert_eq!(state.suggestion_index, Some(2));
    }

    #[test]
    fn suggestion_selection_is_disabled_for_exact_match() {
        let mut state = ConsoleState {
            buf: "scene.inspect".to_string(),
            suggestion_index: Some(0),
            ..Default::default()
        };
        let cache = ConsoleCache {
            predictions_cache: vec!["scene.inspect".to_string()],
            prediction_matches_buffer: true,
            ..Default::default()
        };

        assert!(!move_suggestion_selection(&mut state, &cache, 1));
        assert_eq!(state.suggestion_index, None);
    }

    #[test]
    fn console_configuration_clone_preserves_configuration_fields() {
        let config = ConsoleConfiguration {
            left_pos: 23.0,
            top_pos: 45.0,
            height: 321.0,
            width: 654.0,
            history_size: 73,
            symbol: "$ ".to_string(),
            collapsible: false,
            title_name: "Farmer Console".to_string(),
            resizable: false,
            moveable: false,
            show_title_bar: false,
            background_color: Color32::from_black_alpha(180),
            foreground_color: Color32::YELLOW,
            num_suggestions: 9,
            suggestion_background_color: Color32::from_black_alpha(240),
            suggestion_border_color: Color32::from_gray(120),
            suggestion_selected_background_color: Color32::from_rgb(32, 64, 96),
            ..Default::default()
        };

        let cloned = config.clone();

        assert_eq!(cloned.left_pos, config.left_pos);
        assert_eq!(cloned.top_pos, config.top_pos);
        assert_eq!(cloned.height, config.height);
        assert_eq!(cloned.width, config.width);
        assert_eq!(cloned.history_size, config.history_size);
        assert_eq!(cloned.symbol, config.symbol);
        assert_eq!(cloned.collapsible, config.collapsible);
        assert_eq!(cloned.title_name, config.title_name);
        assert_eq!(cloned.resizable, config.resizable);
        assert_eq!(cloned.moveable, config.moveable);
        assert_eq!(cloned.show_title_bar, config.show_title_bar);
        assert_eq!(cloned.background_color, config.background_color);
        assert_eq!(cloned.foreground_color, config.foreground_color);
        assert_eq!(cloned.num_suggestions, config.num_suggestions);
        assert_eq!(
            cloned.suggestion_background_color,
            config.suggestion_background_color
        );
        assert_eq!(
            cloned.suggestion_border_color,
            config.suggestion_border_color
        );
        assert_eq!(
            cloned.suggestion_selected_background_color,
            config.suggestion_selected_background_color
        );
    }

    #[test]
    fn suggestions_popup_is_hidden_without_predictions() {
        let state = ConsoleState {
            buf: "scene.load --x 1".to_string(),
            ..Default::default()
        };
        let cache = ConsoleCache {
            predictions_cache: Vec::new(),
            prediction_matches_buffer: false,
            ..Default::default()
        };

        assert!(!should_show_suggestions_popup(true, &state, &cache));
    }
}
