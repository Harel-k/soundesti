//! The central command registry.
//!
//! Every action a user can trigger (from a shortcut, menu, toolbar, or script) is a
//! [`Command`] with a stable string ID. Shortcut profiles refer to these IDs, so
//! key bindings never live in UI code.
//!
//! The list mirrors the keyboard commands documented in the FL Studio manual's
//! shortcut page (see docs/SHORTCUT_MATRIX.md), plus Soundesti-only commands. A
//! command existing here does not mean it is implemented — the app reports which
//! commands it handles, and the shortcut matrix records the status.

macro_rules! commands {
    ($( $variant:ident => $id:literal, $cat:ident, $label:literal; )*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Command { $( $variant, )* }

        impl Command {
            pub const ALL: &'static [Command] = &[ $( Command::$variant, )* ];

            pub fn id(self) -> &'static str {
                match self { $( Command::$variant => $id, )* }
            }

            pub fn label(self) -> &'static str {
                match self { $( Command::$variant => $label, )* }
            }

            pub fn category(self) -> Category {
                match self { $( Command::$variant => Category::$cat, )* }
            }

            pub fn from_id(id: &str) -> Option<Command> {
                match id { $( $id => Some(Command::$variant), )* _ => None }
            }
        }
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    File,
    Edit,
    Transport,
    View,
    Pattern,
    ChannelRack,
    PianoRoll,
    Playlist,
    Mixer,
    Browser,
    Tool,
    Typing,
    Help,
}

commands! {
    // File
    NewProject => "file.new", File, "New project";
    Open => "file.open", File, "Open project";
    OpenRecent => "file.open_recent", File, "Open recent file (0-9)";
    Save => "file.save", File, "Save";
    SaveAs => "file.save_as", File, "Save as";
    SaveNewVersion => "file.save_new_version", File, "Save new version";
    ExportWav => "file.export_wav", File, "Export WAV";
    ExportMp3 => "file.export_mp3", File, "Export MP3";
    ExportMidi => "file.export_midi", File, "Export MIDI";

    // Global edit / undo
    UndoToggle => "edit.undo_toggle", Edit, "Undo/redo last edit";
    UndoStep => "edit.undo_step", Edit, "Undo step by step";
    Redo => "edit.redo", Edit, "Redo";

    // Transport / recording
    PlayStop => "transport.play_stop", Transport, "Start/stop playback";
    PlayPause => "transport.play_pause", Transport, "Start/pause playback";
    ToggleSongMode => "transport.toggle_song_mode", Transport, "Switch pattern/song mode";
    ToggleRecord => "transport.toggle_record", Transport, "Switch recording on/off";
    ToggleMetronome => "transport.toggle_metronome", Transport, "Toggle metronome";
    TogglePrecount => "transport.toggle_precount", Transport, "Toggle recording metronome precount";
    ToggleWaitForInput => "transport.toggle_wait_for_input", Transport, "Toggle wait for input to start recording";
    ToggleStepEdit => "transport.toggle_step_edit", Transport, "Toggle step edit mode";
    ToggleBlendNotes => "transport.toggle_blend_notes", Transport, "Toggle blend notes";
    Panic => "transport.panic", Transport, "Stop sound (panic)";
    FastForward => "transport.fast_forward", Transport, "Fast forward";
    NextBar => "transport.next_bar", Transport, "Next bar (song mode)";
    PreviousBar => "transport.previous_bar", Transport, "Previous bar (song mode)";
    PlayheadToStart => "transport.playhead_to_start", Transport, "Move playback marker to start";
    ToggleTypingKeyboard => "transport.toggle_typing_keyboard", Typing, "Typing keyboard to piano keyboard";
    TypingOctave => "typing.octave", Typing, "Change typing keyboard octave";

    // Snap
    ToggleSnapNone => "snap.toggle_none", Edit, "Toggle line/none snap";
    ToggleGlobalSnap => "snap.toggle_global", Edit, "Toggle global snap";

    // Windows / view
    ToggleChannelRack => "view.toggle_channel_rack", View, "Toggle Channel Rack";
    TogglePianoRoll => "view.toggle_piano_roll", View, "Toggle Piano roll";
    TogglePlaylist => "view.toggle_playlist", View, "Toggle Playlist";
    ToggleMixer => "view.toggle_mixer", View, "Show/hide Mixer";
    PluginPicker => "view.plugin_picker", View, "Open Plugin Picker";
    ToggleBrowser => "view.toggle_browser", View, "Show/hide Browser";
    ToggleMidiSettings => "view.toggle_midi_settings", View, "Show/hide MIDI settings";
    ToggleSongInfo => "view.toggle_song_info", View, "Show/hide song info";
    CloseAllWindows => "view.close_all", View, "Close all windows";
    CloseWindow => "view.close_window", View, "Close focused window";
    CycleWindows => "view.cycle_windows", View, "Cycle nested windows";
    ArrangeWindows => "view.arrange_windows", View, "(Re)arrange windows";
    TogglePlaylistMaximized => "view.toggle_playlist_maximized", View, "Toggle max/min Playlist";
    FocusPlaylist => "view.focus_playlist", View, "Focus Playlist";
    ProjectPicker => "view.project_picker", View, "Open Project Picker";
    ToolSelector => "view.tool_selector", View, "Tool selector for active window";
    ZoomIn => "view.zoom_in", View, "Zoom in";
    ZoomOut => "view.zoom_out", View, "Zoom out";
    ZoomLevel => "view.zoom_level", View, "Horizontal zoom level";
    ZoomAll => "view.zoom_all", View, "Horizontal zoom, show all";
    ZoomToSelection => "view.zoom_selection", View, "Zoom to selection";
    FitPerformanceZone => "view.fit_performance", View, "Fit Performance Zone";
    CenterOnPlayhead => "view.center_on_playhead", View, "Center view on playhead";
    GridColor => "view.grid_color", View, "Grid color";
    Help => "help.open", Help, "Help";

    // Pattern selector
    SelectPattern => "pattern.select", Pattern, "Select pattern 1-9";
    NextPattern => "pattern.next", Pattern, "Next pattern";
    PreviousPattern => "pattern.previous", Pattern, "Previous pattern";
    NextEmptyPattern => "pattern.next_empty", Pattern, "Next empty pattern";
    NextEmptyPatternNamed => "pattern.next_empty_named", Pattern, "Next empty pattern with naming";
    RenamePattern => "pattern.rename", Pattern, "Rename selected pattern";

    // Shared editing (Playlist / Piano roll / Channel Rack act on their own selection)
    SelectAll => "edit.select_all", Edit, "Select all";
    Deselect => "edit.deselect", Edit, "Deselect";
    InvertSelection => "edit.invert_selection", Edit, "Invert selection";
    Copy => "edit.copy", Edit, "Copy selection";
    Cut => "edit.cut", Edit, "Cut selection";
    Paste => "edit.paste", Edit, "Paste selection";
    DeleteSelection => "edit.delete_selection", Edit, "Delete selected";
    DuplicateRight => "edit.duplicate_right", Edit, "Duplicate selection to the right";
    MoveSelectionLeft => "edit.move_left", Edit, "Move selection left";
    MoveSelectionRight => "edit.move_right", Edit, "Move selection right";
    MoveSelectionUp => "edit.move_up", Edit, "Move selection up";
    MoveSelectionDown => "edit.move_down", Edit, "Move selection down";
    NudgeLeft => "edit.nudge_left", Edit, "Nudge left";
    NudgeRight => "edit.nudge_right", Edit, "Nudge right";
    NudgeUp => "edit.nudge_up", Edit, "Nudge up";
    NudgeDown => "edit.nudge_down", Edit, "Nudge down";
    TransposeOctaveUp => "edit.transpose_octave_up", Edit, "Transpose selection up 1 octave";
    TransposeOctaveDown => "edit.transpose_octave_down", Edit, "Transpose selection down 1 octave";
    MuteSelection => "edit.mute_selection", Edit, "Mute selection";
    UnmuteSelection => "edit.unmute_selection", Edit, "Unmute selection";
    Group => "edit.group", Edit, "Group selected";
    Ungroup => "edit.ungroup", Edit, "Ungroup selected";
    SelectByColor => "edit.select_by_color", Edit, "Select by color";
    SelectTimeBefore => "edit.select_time_before", Edit, "Select time before selection";
    SelectTimeAfter => "edit.select_time_after", Edit, "Select time after selection";
    SelectTimeAround => "edit.select_time_around", Edit, "Select time around selection";
    DeleteSpace => "edit.delete_space", Edit, "Delete space equal to selection";
    InsertSpace => "edit.insert_space", Edit, "Insert space equal to selection";
    ToggleResizeFromLeft => "edit.toggle_resize_from_left", Edit, "Allow resizing from left";
    SetTimeSignature => "edit.set_time_signature", Edit, "Set a time signature";

    // Editor tools
    ToolDraw => "tool.draw", Tool, "Draw tool (pencil)";
    ToolPaint => "tool.paint", Tool, "Paint tool";
    ToolDelete => "tool.delete", Tool, "Delete tool";
    ToolMute => "tool.mute", Tool, "Mute tool";
    ToolSlice => "tool.slice", Tool, "Slice tool";
    ToolSelect => "tool.select", Tool, "Select tool";
    ToolZoom => "tool.zoom", Tool, "Zoom tool";
    ToolPlayback => "tool.playback", Tool, "Playback tool";
    ToolSlip => "tool.slip", Tool, "Slip edit tool";

    // Channel Rack
    ChannelToggleMute => "channel.toggle_mute", ChannelRack, "Mute/unmute channel 1-10";
    ChannelToggleSolo => "channel.toggle_solo", ChannelRack, "Solo/unsolo channel 1-10";
    ChannelSelectNext => "channel.select_next", ChannelRack, "Select channel below";
    ChannelSelectPrevious => "channel.select_previous", ChannelRack, "Select channel above";
    ChannelExtendSelectionDown => "channel.extend_selection_down", ChannelRack, "Move channel selection down";
    ChannelExtendSelectionUp => "channel.extend_selection_up", ChannelRack, "Move channel selection up";
    ChannelNextGroup => "channel.next_group", ChannelRack, "Select next channel group";
    ChannelPreviousGroup => "channel.previous_group", ChannelRack, "Select previous channel group";
    ChannelClone => "channel.clone", ChannelRack, "Clone selected channels";
    ChannelDelete => "channel.delete", ChannelRack, "Delete selected channels";
    ChannelMoveUp => "channel.move_up", ChannelRack, "Move selected channels up";
    ChannelMoveDown => "channel.move_down", ChannelRack, "Move selected channels down";
    ChannelGroup => "channel.group", ChannelRack, "Group selected channels";
    ChannelZip => "channel.zip", ChannelRack, "Zip selected channels";
    ChannelUnzip => "channel.unzip", ChannelRack, "Unzip selected channels";
    ChannelToggleMixerSelectors => "channel.toggle_mixer_selectors", ChannelRack, "Toggle mixer track selectors";
    ChannelForceRestretch => "channel.force_restretch", ChannelRack, "Force restretch of audio clip channels";
    ChannelCopySteps => "channel.copy_steps", ChannelRack, "Copy channel steps/score";
    ChannelCutSteps => "channel.cut_steps", ChannelRack, "Cut channel steps/score";
    ChannelPasteSteps => "channel.paste_steps", ChannelRack, "Paste channel steps/score";
    ChannelRouteToFreeTracks => "channel.route_to_free_tracks", ChannelRack, "Route selected channels to free mixer tracks";
    ChannelShiftStepsLeft => "channel.shift_steps_left", ChannelRack, "Shift steps left";
    ChannelShiftStepsRight => "channel.shift_steps_right", ChannelRack, "Shift steps right";

    // Piano roll
    PrNextEventProperty => "pianoroll.next_event_property", PianoRoll, "Show next property in event lane";
    PrCycleNoteProperties => "pianoroll.cycle_note_properties", PianoRoll, "Cycle through note properties";
    PrChannelAbove => "pianoroll.channel_above", PianoRoll, "Select the channel above";
    PrChannelBelow => "pianoroll.channel_below", PianoRoll, "Select the channel below";
    PrChannelAboveWithNotes => "pianoroll.channel_above_with_notes", PianoRoll, "Select the channel above with note data";
    PrChannelBelowWithNotes => "pianoroll.channel_below_with_notes", PianoRoll, "Select the channel below with note data";
    PrNextChannel => "pianoroll.next_channel", PianoRoll, "Select the next channel";
    PrNextChannelWithNotes => "pianoroll.next_channel_with_notes", PianoRoll, "Select next channel with notes";
    PrToggleKeyboardView => "pianoroll.toggle_keyboard_view", PianoRoll, "Keyboard view mode";
    PrTogglePortamento => "pianoroll.toggle_portamento", PianoRoll, "Portamento (toggle)";
    PrToggleSlide => "pianoroll.toggle_slide", PianoRoll, "Slide note (toggle)";
    PrArpeggiator => "pianoroll.arpeggiator", PianoRoll, "Arpeggiator";
    PrNoteHelpers => "pianoroll.note_helpers", PianoRoll, "View note helpers";
    PrColorSelection => "pianoroll.color_selection", PianoRoll, "Change color of selected notes";
    PrRiffMachine => "pianoroll.riff_machine", PianoRoll, "Riff machine";
    PrFlam => "pianoroll.flam", PianoRoll, "Flam";
    PrLimit => "pianoroll.limit", PianoRoll, "Limit tool";
    PrArticulate => "pianoroll.articulate", PianoRoll, "Articulate tool";
    PrWaveformHelper => "pianoroll.waveform_helper", PianoRoll, "Switch waveform helper view";
    PrLfo => "pianoroll.lfo", PianoRoll, "LFO tool";
    PrQuantize => "pianoroll.quantize", PianoRoll, "Quantize";
    PrQuickQuantize => "pianoroll.quick_quantize", PianoRoll, "Quick quantize";
    PrRandomize => "pianoroll.randomize", PianoRoll, "Randomize tool";
    PrStrum => "pianoroll.strum", PianoRoll, "Strum tool";
    PrChop => "pianoroll.chop", PianoRoll, "Chop tool";
    PrQuickChop => "pianoroll.quick_chop", PianoRoll, "Quick chop";
    PrToggleGhostChannels => "pianoroll.toggle_ghost_channels", PianoRoll, "Ghost channels on/off";
    PrClawMachine => "pianoroll.claw_machine", PianoRoll, "Claw machine tool";
    PrScaleLevels => "pianoroll.scale_levels", PianoRoll, "Scale levels tool";
    PrScoreFlipper => "pianoroll.score_flipper", PianoRoll, "Score flipper tool";
    PrGlue => "pianoroll.glue", PianoRoll, "Glue selected notes";
    PrInsertControllerValue => "pianoroll.insert_controller_value", PianoRoll, "Insert current controller value";
    PrQuickLegato => "pianoroll.quick_legato", PianoRoll, "Quick legato";
    PrImportMidi => "pianoroll.import_midi", PianoRoll, "Import MIDI file";
    PrPasteMidiClipboard => "pianoroll.paste_midi_clipboard", PianoRoll, "Paste from MIDI clipboard";
    PrDiscardLengths => "pianoroll.discard_lengths", PianoRoll, "Discard selected note lengths";
    PrSelectRandomGroups => "pianoroll.select_random_groups", PianoRoll, "Select groups of notes at random";
    PrChordOff => "pianoroll.chord_off", PianoRoll, "Turn off selected chord";
    PrSelectRandom => "pianoroll.select_random", PianoRoll, "Select at random";

    // Playlist
    PlZoomPerformanceClips => "playlist.zoom_performance_clips", Playlist, "Zoom on Performance Clips";
    PlTransposeDown => "playlist.transpose_down", Playlist, "Transpose down";
    PlTransposeUp => "playlist.transpose_up", Playlist, "Transpose up";
    PlReverse => "playlist.reverse", Playlist, "Reverse (audio clips)";
    PlSliceAtCursor => "playlist.slice_at_cursor", Playlist, "Slice clip at cursor position";
    PlNextMarker => "playlist.next_marker", Playlist, "Jump to next song marker";
    PlPreviousMarker => "playlist.previous_marker", Playlist, "Jump to previous song marker";
    PlPickerPanel => "playlist.picker_panel", Playlist, "Open the Picker Panel";
    PlAddTimeMarker => "playlist.add_time_marker", Playlist, "Add time marker";
    PlConsolidate => "playlist.consolidate", Playlist, "Consolidate selected clips from first clip";
    PlConsolidateFromStart => "playlist.consolidate_from_start", Playlist, "Consolidate selected clips from Playlist start";
    PlTogglePerformanceMode => "playlist.toggle_performance_mode", Playlist, "Toggle Performance Mode";
    PlSelectBySource => "playlist.select_by_source", Playlist, "Select all clips by source";
    PlSwitchStretchMode => "playlist.switch_stretch_mode", Playlist, "Switch stretch mode";
    PlQuickQuantizeStarts => "playlist.quick_quantize_starts", Playlist, "Quick quantize clip start times";

    // Mixer
    MixerSolo => "mixer.solo", Mixer, "Solo current track";
    MixerAltSolo => "mixer.alt_solo", Mixer, "Solo track and all tracks routed to/from it";
    MixerSelectLinkedChannels => "mixer.select_linked_channels", Mixer, "Select channels linked to selected track";
    MixerMoveLeft => "mixer.move_left", Mixer, "Move selected tracks left";
    MixerMoveRight => "mixer.move_right", Mixer, "Move selected tracks right";
    MixerRenderArmed => "mixer.render_armed", Mixer, "Render armed tracks to .wav";
    MixerToggleWaveView => "mixer.toggle_wave_view", Mixer, "Toggle peak meter wave view";
    MixerSelectAll => "mixer.select_all", Mixer, "Select all mixer tracks";
    MixerLinkChannels => "mixer.link_channels", Mixer, "Link selected channels to selected track";
    MixerLinkChannelsFrom => "mixer.link_channels_from", Mixer, "Link selected channels starting from selected track";
    MixerSaveTrackState => "mixer.save_track_state", Mixer, "Save mixer track state";
    MixerToggleProperties => "mixer.toggle_properties", Mixer, "Show/hide mixer track properties";
    MixerRename => "mixer.rename", Mixer, "Rename selected mixer track";

    // Browser
    BrowserReplaceChannel => "browser.replace_channel", Browser, "Replace channel with Browser item";
    BrowserPreviewPrevious => "browser.preview_previous", Browser, "Step to previous item with preview";
    BrowserPreviewNext => "browser.preview_next", Browser, "Step to next item with preview";
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique_and_round_trip() {
        let mut seen = HashSet::new();
        for &c in Command::ALL {
            assert!(seen.insert(c.id()), "duplicate id {}", c.id());
            assert_eq!(Command::from_id(c.id()), Some(c));
            assert!(c.id().contains('.'), "{} should be namespaced", c.id());
            assert!(!c.label().is_empty());
        }
        assert!(Command::from_id("nope.nothing").is_none());
    }
}
