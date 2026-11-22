use std::sync::Arc;

use gpui::{FontStyle, FontWeight, HighlightStyle, Hsla, WindowBackgroundAppearance, hsla};

use crate::{
    AccentColors, Appearance, DEFAULT_DARK_THEME, PlayerColors, StatusColors,
    StatusColorsRefinement, SyntaxTheme, SystemColors, Theme, ThemeColors, ThemeColorsRefinement,
    ThemeFamily, ThemeStyles, default_color_scales,
};

/// The default theme family for Obake.
///
/// This is used to construct the default theme fallback values, as well as to
/// have a theme available at compile time for tests.
pub fn obake_default_themes() -> ThemeFamily {
    ThemeFamily {
        id: "obake-default".to_string(),
        name: "Obake Default".into(),
        author: "".into(),
        themes: vec![obake_default_dark()],
        scales: default_color_scales(),
    }
}

// If a theme customizes a foreground version of a status color, but does not
// customize the background color, then use a partly-transparent version of the
// foreground color for the background color.
/// Applies default status color backgrounds from their foreground counterparts.
pub fn apply_status_color_defaults(status: &mut StatusColorsRefinement) {
    for (fg_color, bg_color) in [
        (&status.deleted, &mut status.deleted_background),
        (&status.created, &mut status.created_background),
        (&status.modified, &mut status.modified_background),
        (&status.conflict, &mut status.conflict_background),
        (&status.error, &mut status.error_background),
        (&status.hidden, &mut status.hidden_background),
    ] {
        if bg_color.is_none()
            && let Some(fg_color) = fg_color
        {
            *bg_color = Some(fg_color.opacity(0.25));
        }
    }
}

/// Applies default theme color values derived from player colors.
pub fn apply_theme_color_defaults(
    theme_colors: &mut ThemeColorsRefinement,
    player_colors: &PlayerColors,
) {
    if theme_colors.element_selection_background.is_none() {
        let mut selection = player_colors.local().selection;
        if selection.a == 1.0 {
            selection.a = 0.25;
        }
        theme_colors.element_selection_background = Some(selection);
    }
}

pub(crate) fn obake_default_dark() -> Theme {
    let bg = hsla(0., 0., 14.5 / 100., 1.);
    let editor = hsla(0., 0., 17.2 / 100., 1.);
    let elevated_surface = hsla(0., 0., 16.0 / 100., 1.);
    let hover = hsla(0., 0., 25.2 / 100., 1.0);

    let blue = hsla(0., 0., 65.3 / 100., 1.0);
    let gray = hsla(0., 0., 38.7 / 100., 1.0);
    let green = hsla(0., 0., 70.4 / 100., 1.0);
    let orange = hsla(0., 0., 65.3 / 100., 1.0);
    let purple = hsla(0., 0., 58.7 / 100., 1.0);
    let red = hsla(0., 0., 58.5 / 100., 1.0);
    let teal = hsla(0., 0., 64.8 / 100., 1.0);
    let yellow = hsla(0., 0., 77.4 / 100., 1.0);

    const ADDED_COLOR: Hsla = Hsla {
        h: 0.,
        s: 0.,
        l: 0.523,
        a: 1.0,
    };
    const WORD_ADDED_COLOR: Hsla = Hsla {
        h: 0.,
        s: 0.,
        l: 0.523,
        a: 0.35,
    };
    const MODIFIED_COLOR: Hsla = Hsla {
        h: 0.,
        s: 0.,
        l: 0.697,
        a: 1.0,
    };
    const REMOVED_COLOR: Hsla = Hsla {
        h: 0.,
        s: 0.,
        l: 0.245,
        a: 1.0,
    };
    const WORD_DELETED_COLOR: Hsla = Hsla {
        h: 0.,
        s: 0.,
        l: 0.245,
        a: 0.80,
    };

    let player = PlayerColors::dark();
    Theme {
        id: "one_dark".to_string(),
        name: DEFAULT_DARK_THEME.into(),
        appearance: Appearance::Dark,
        styles: ThemeStyles {
            window_background_appearance: WindowBackgroundAppearance::Opaque,
            system: SystemColors::default(),
            accents: AccentColors(Arc::from(vec![
                blue, orange, purple, teal, red, green, yellow,
            ])),
            colors: ThemeColors {
                border: hsla(0., 0., 11.3 / 100., 1.),
                border_variant: hsla(0., 0., 23.9 / 100., 1.),
                border_focused: hsla(0., 0., 55.3 / 100., 1.),
                border_selected: hsla(0., 0., 55.7 / 100., 1.0),
                border_transparent: SystemColors::default().transparent,
                border_disabled: hsla(0., 0., 32.1 / 100., 1.0),
                elevated_surface_background: elevated_surface,
                surface_background: bg,
                background: bg,
                element_background: hsla(0., 0., 19.8 / 100., 1.0),
                element_hover: hover,
                element_active: hsla(0., 0., 19.2 / 100., 1.0),
                element_selected: hsla(0., 0., 24.8 / 100., 1.0),
                element_disabled: SystemColors::default().transparent,
                element_selection_background: player.local().selection.alpha(0.25),
                drop_target_background: hsla(0., 0., 20.8 / 100., 1.0),
                drop_target_border: hsla(0., 0., 85.4 / 100., 1.0),
                ghost_element_background: SystemColors::default().transparent,
                ghost_element_hover: hover,
                ghost_element_active: hsla(0., 0., 19.2 / 100., 1.0),
                ghost_element_selected: hsla(0., 0., 24.8 / 100., 1.0),
                ghost_element_disabled: SystemColors::default().transparent,
                text: hsla(0., 0., 85.4 / 100., 1.0),
                text_muted: hsla(0., 0., 45.0 / 100., 1.0),
                text_placeholder: hsla(0., 0., 43.4 / 100., 1.0),
                text_disabled: hsla(0., 0., 43.4 / 100., 1.0),
                text_accent: hsla(0., 0., 55.7 / 100., 1.0),
                icon: hsla(0., 0., 85.5 / 100., 1.0),
                icon_muted: hsla(0., 0., 64.6 / 100., 1.0),
                icon_disabled: hsla(0., 0., 44.6 / 100., 1.0),
                icon_placeholder: hsla(0., 0., 44.6 / 100., 1.0),
                icon_accent: blue,
                debugger_accent: red,
                status_bar_background: bg,
                title_bar_background: bg,
                title_bar_inactive_background: bg,
                toolbar_background: editor,
                tab_bar_background: bg,
                tab_inactive_background: bg,
                tab_active_background: editor,
                search_match_background: bg,
                search_active_match_background: bg,

                editor_background: editor,
                editor_gutter_background: editor,
                editor_subheader_background: bg,
                editor_active_line_background: hsla(0., 0., 19.2 / 100., 1.0),
                editor_highlighted_line_background: hsla(0., 0., 65.3 / 100., 0.1),
                editor_debugger_active_line_background: hsla(0., 0., 65.3 / 100., 0.2),
                editor_line_number: hsla(0., 0., 32.5 / 100., 1.0),
                editor_active_line_number: hsla(0., 0., 48.8 / 100., 1.0),
                editor_hover_line_number: hsla(0., 0., 56.0 / 100., 1.0),
                editor_invisible: hsla(0., 0., 32.5 / 100., 1.0),
                editor_wrap_guide: hsla(0., 0., 23.9 / 100., 1.),
                editor_active_wrap_guide: hsla(0., 0., 23.9 / 100., 1.),
                editor_indent_guide: hsla(0., 0., 23.9 / 100., 1.),
                editor_indent_guide_active: hsla(0., 0., 11.3 / 100., 1.),
                editor_document_highlight_read_background: hsla(0., 0., 65.3 / 100., 0.2),
                editor_document_highlight_write_background: gpui::red(),
                editor_document_highlight_bracket_background: gpui::green(),
                editor_diff_hunk_added_background: ADDED_COLOR.opacity(0.12),
                editor_diff_hunk_added_hollow_background: ADDED_COLOR.opacity(0.06),
                editor_diff_hunk_added_hollow_border: ADDED_COLOR.opacity(0.36),
                editor_diff_hunk_deleted_background: REMOVED_COLOR.opacity(0.12),
                editor_diff_hunk_deleted_hollow_background: REMOVED_COLOR.opacity(0.06),
                editor_diff_hunk_deleted_hollow_border: REMOVED_COLOR.opacity(0.36),

                terminal_background: bg,
                // todo("Use one colors for terminal")
                terminal_ansi_background: crate::black().dark().step_12(),
                terminal_foreground: crate::white().dark().step_12(),
                terminal_bright_foreground: crate::white().dark().step_11(),
                terminal_dim_foreground: crate::white().dark().step_10(),
                terminal_ansi_black: crate::black().dark().step_12(),
                terminal_ansi_red: crate::red().dark().step_11(),
                terminal_ansi_green: crate::green().dark().step_11(),
                terminal_ansi_yellow: crate::yellow().dark().step_11(),
                terminal_ansi_blue: crate::blue().dark().step_11(),
                terminal_ansi_magenta: crate::violet().dark().step_11(),
                terminal_ansi_cyan: crate::cyan().dark().step_11(),
                terminal_ansi_white: crate::neutral().dark().step_12(),
                terminal_ansi_bright_black: crate::black().dark().step_11(),
                terminal_ansi_bright_red: crate::red().dark().step_10(),
                terminal_ansi_bright_green: crate::green().dark().step_10(),
                terminal_ansi_bright_yellow: crate::yellow().dark().step_10(),
                terminal_ansi_bright_blue: crate::blue().dark().step_10(),
                terminal_ansi_bright_magenta: crate::violet().dark().step_10(),
                terminal_ansi_bright_cyan: crate::cyan().dark().step_10(),
                terminal_ansi_bright_white: crate::neutral().dark().step_11(),
                terminal_ansi_dim_black: crate::black().dark().step_10(),
                terminal_ansi_dim_red: crate::red().dark().step_9(),
                terminal_ansi_dim_green: crate::green().dark().step_9(),
                terminal_ansi_dim_yellow: crate::yellow().dark().step_9(),
                terminal_ansi_dim_blue: crate::blue().dark().step_9(),
                terminal_ansi_dim_magenta: crate::violet().dark().step_9(),
                terminal_ansi_dim_cyan: crate::cyan().dark().step_9(),
                terminal_ansi_dim_white: crate::neutral().dark().step_10(),
                panel_background: bg,
                panel_focused_border: blue,
                panel_indent_guide: hsla(0., 0., 23.9 / 100., 1.),
                panel_indent_guide_hover: hsla(0., 0., 11.3 / 100., 1.),
                panel_indent_guide_active: hsla(0., 0., 11.3 / 100., 1.),
                panel_overlay_background: bg,
                panel_overlay_hover: hover,
                pane_focused_border: blue,
                pane_group_border: hsla(0., 0., 11.3 / 100., 1.),
                scrollbar_thumb_background: gpui::transparent_black(),
                scrollbar_thumb_hover_background: hover,
                scrollbar_thumb_active_background: hsla(0., 0., 25.2 / 100., 1.0),
                scrollbar_thumb_border: hsla(0., 0., 23.9 / 100., 1.),
                scrollbar_track_background: gpui::transparent_black(),
                scrollbar_track_border: hsla(0., 0., 23.9 / 100., 1.),
                minimap_thumb_background: hsla(0., 0., 25.2 / 100., 0.7),
                minimap_thumb_hover_background: hsla(0., 0., 25.2 / 100., 0.7),
                minimap_thumb_active_background: hsla(0., 0., 25.2 / 100., 0.7),
                minimap_thumb_border: hsla(0., 0., 23.9 / 100., 1.),
                editor_foreground: hsla(0., 0., 69.7 / 100., 1.),
                editor_code_lens_foreground: None,
                link_text_hover: blue,
                version_control_added: ADDED_COLOR,
                version_control_deleted: REMOVED_COLOR,
                version_control_modified: MODIFIED_COLOR,
                version_control_renamed: MODIFIED_COLOR,
                version_control_conflict: crate::orange().light().step_12(),
                version_control_ignored: crate::gray().light().step_12(),
                version_control_word_added: WORD_ADDED_COLOR,
                version_control_word_deleted: WORD_DELETED_COLOR,
                version_control_conflict_marker_ours: crate::green().light().step_12().alpha(0.5),
                version_control_conflict_marker_theirs: crate::blue().light().step_12().alpha(0.5),

                vim_normal_background: SystemColors::default().transparent,
                vim_insert_background: SystemColors::default().transparent,
                vim_replace_background: SystemColors::default().transparent,
                vim_visual_background: SystemColors::default().transparent,
                vim_visual_line_background: SystemColors::default().transparent,
                vim_visual_block_background: SystemColors::default().transparent,
                vim_yank_background: hsla(0., 0., 65.3 / 100., 0.2),
                vim_helix_jump_label_foreground: red,
                vim_helix_normal_background: SystemColors::default().transparent,
                vim_helix_select_background: SystemColors::default().transparent,
                vim_normal_foreground: SystemColors::default().transparent,
                vim_insert_foreground: SystemColors::default().transparent,
                vim_replace_foreground: SystemColors::default().transparent,
                vim_visual_foreground: SystemColors::default().transparent,
                vim_visual_line_foreground: SystemColors::default().transparent,
                vim_visual_block_foreground: SystemColors::default().transparent,
                vim_helix_normal_foreground: SystemColors::default().transparent,
                vim_helix_select_foreground: SystemColors::default().transparent,
            },
            status: StatusColors {
                conflict: yellow,
                conflict_background: yellow,
                conflict_border: yellow,
                created: green,
                created_background: green,
                created_border: green,
                deleted: red,
                deleted_background: red,
                deleted_border: red,
                error: red,
                error_background: red,
                error_border: red,
                hidden: gray,
                hidden_background: gray,
                hidden_border: gray,
                hint: blue,
                hint_background: blue,
                hint_border: blue,
                ignored: gray,
                ignored_background: gray,
                ignored_border: gray,
                info: blue,
                info_background: blue,
                info_border: blue,
                modified: yellow,
                modified_background: yellow,
                modified_border: yellow,
                predictive: gray,
                predictive_background: gray,
                predictive_border: gray,
                renamed: blue,
                renamed_background: blue,
                renamed_border: blue,
                success: green,
                success_background: green,
                success_border: green,
                unreachable: gray,
                unreachable_background: gray,
                unreachable_border: gray,
                warning: yellow,
                warning_background: yellow,
                warning_border: yellow,
            },
            player,
            syntax: Arc::new(SyntaxTheme::new(vec![
                ("attribute".into(), purple.into()),
                ("boolean".into(), orange.into()),
                ("comment".into(), gray.into()),
                ("comment.doc".into(), gray.into()),
                ("constant".into(), yellow.into()),
                ("constructor".into(), blue.into()),
                ("embedded".into(), HighlightStyle::default()),
                (
                    "emphasis".into(),
                    HighlightStyle {
                        font_style: Some(FontStyle::Italic),
                        ..HighlightStyle::default()
                    },
                ),
                (
                    "emphasis.strong".into(),
                    HighlightStyle {
                        font_weight: Some(FontWeight::BOLD),
                        ..HighlightStyle::default()
                    },
                ),
                ("enum".into(), teal.into()),
                ("function".into(), blue.into()),
                ("function.method".into(), blue.into()),
                ("function.definition".into(), blue.into()),
                ("hint".into(), blue.into()),
                ("keyword".into(), purple.into()),
                ("label".into(), HighlightStyle::default()),
                ("link_text".into(), blue.into()),
                (
                    "link_uri".into(),
                    HighlightStyle {
                        color: Some(teal),
                        font_style: Some(FontStyle::Italic),
                        ..HighlightStyle::default()
                    },
                ),
                ("number".into(), orange.into()),
                ("operator".into(), HighlightStyle::default()),
                ("predictive".into(), HighlightStyle::default()),
                ("preproc".into(), purple.into()),
                ("primary".into(), HighlightStyle::default()),
                ("property".into(), red.into()),
                ("punctuation".into(), HighlightStyle::default()),
                ("punctuation.bracket".into(), HighlightStyle::default()),
                ("punctuation.delimiter".into(), HighlightStyle::default()),
                ("punctuation.list_marker".into(), HighlightStyle::default()),
                ("punctuation.special".into(), HighlightStyle::default()),
                ("string".into(), green.into()),
                ("string.escape".into(), HighlightStyle::default()),
                ("string.regex".into(), red.into()),
                ("string.special".into(), HighlightStyle::default()),
                ("string.special.symbol".into(), HighlightStyle::default()),
                ("tag".into(), HighlightStyle::default()),
                ("text.literal".into(), HighlightStyle::default()),
                ("title".into(), HighlightStyle::default()),
                ("type".into(), teal.into()),
                ("variable".into(), HighlightStyle::default()),
                ("variable.special".into(), red.into()),
                ("variant".into(), HighlightStyle::default()),
                ("diff.plus".into(), green.into()),
                ("diff.minus".into(), red.into()),
            ])),
        },
    }
}
