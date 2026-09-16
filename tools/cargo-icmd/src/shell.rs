//! The guide's application shell: header, responsive index, document column,
//! search overlay, and global theme controls.
//!
//! Chapter content lives in [`crate::chapters`]; this module owns only chrome,
//! navigation state, and the responsive decisions that choose between the
//! expanded index, the compact rail, and a content-only layout.

use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyModifiers};
use icmd::theme::{Theme, ThemeMode, ThemePreset};
use icmd::{
    Align, AxisPosition, BorderKind, ButtonVariant, Component, ComponentContext, Dimension, Edges,
    ElementRef, ElementSnapshot, Justify, KeyboardEvent, Layout, Node, Percent, Props,
    RuntimeConfig, ScrollAxes, ScrollOffset, ScrollbarVisibility, Span, StateSetter, Text,
    TextAlign, TextOverflow, TextWrap, button, divider, empty, input, kbd, muted, render, row,
    scroll_area, theme_provider, ui, view,
};

use crate::chapters::{ChapterProps, NavBindings, Navigator, breadcrumb, selected_chapter};
use crate::demos::{self, WidthMode as IndexMode};
use crate::metadata;
use crate::search::{HitKind, SearchHit, search};

/// Widest the search overlay is allowed to grow.
const MAX_SEARCH_WIDTH: u16 = 78;
/// Assumed first-frame width before the root element reports its own.
const ASSUMED_WIDTH: u16 = 120;
/// Row count assumed before the terminal reports its own size.
const ASSUMED_HEIGHT: u16 = 24;
/// Tallest the search dialog may grow, so it reads as a card rather than a
/// sheet that covers the page behind it.
const MAX_SEARCH_HEIGHT: u16 = 16;

/// Props that let the shell start in a chosen state.
#[derive(Clone)]
pub(crate) struct ShellProps {
    /// Theme preset used for the first render.
    pub initial_preset: ThemePreset,
    /// Light or dark mode used for the first render.
    pub initial_mode: ThemeMode,
    /// Chapter mounted on the first render.
    pub initial_chapter: usize,
    /// Whether the reader has collapsed the index.
    pub initial_collapsed: bool,
    /// Whether the search overlay starts open.
    pub initial_search: bool,
    /// Query the search overlay starts with.
    pub initial_query: String,
}

impl Default for ShellProps {
    fn default() -> Self {
        Self {
            initial_preset: ThemePreset::Nord,
            initial_mode: ThemeMode::Dark,
            initial_chapter: 0,
            initial_collapsed: false,
            initial_search: false,
            initial_query: String::new(),
        }
    }
}

/// A shell-level keyboard accelerator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shortcut {
    /// Open or close the search overlay.
    ToggleSearch,
    /// Expand or collapse the index.
    ToggleIndex,
    /// Move to the previous chapter.
    PrevChapter,
    /// Move to the next chapter.
    NextChapter,
    /// Move to the previous section.
    PrevSection,
    /// Move to the next section.
    NextSection,
    /// Close the search overlay.
    CloseSearch,
}

/// Maps a key event to a shell accelerator.
///
/// Only modifier chords are accelerators. Bare letters, digits, and arrow keys
/// are deliberately unbound so demonstrated inputs keep receiving them.
pub(crate) fn shell_shortcut(event: &KeyboardEvent) -> Option<Shortcut> {
    let modifiers = event.key.modifiers;
    let control = modifiers.contains(KeyModifiers::CONTROL);
    let alt = modifiers.contains(KeyModifiers::ALT)
        && !modifiers.contains(KeyModifiers::SUPER)
        && !modifiers.contains(KeyModifiers::META);
    if control && alt {
        return None;
    }
    match event.key.code {
        KeyCode::Char('k') if control => Some(Shortcut::ToggleSearch),
        KeyCode::Char('b') if control => Some(Shortcut::ToggleIndex),
        KeyCode::Left if alt => Some(Shortcut::PrevChapter),
        KeyCode::Right if alt => Some(Shortcut::NextChapter),
        KeyCode::Up if alt => Some(Shortcut::PrevSection),
        KeyCode::Down if alt => Some(Shortcut::NextSection),
        KeyCode::Esc if !control && !alt => Some(Shortcut::CloseSearch),
        _ => None,
    }
}

/// Next preset in display order.
fn next_preset(preset: ThemePreset) -> ThemePreset {
    let all = ThemePreset::ALL;
    let position = all
        .iter()
        .position(|candidate| *candidate == preset)
        .unwrap_or(0);
    all[(position + 1) % all.len()]
}

/// Opposite light/dark mode.
fn next_mode(mode: ThemeMode) -> ThemeMode {
    match mode {
        ThemeMode::Dark => ThemeMode::Light,
        ThemeMode::Light => ThemeMode::Dark,
    }
}

/// Every value the shell needs while building one frame.
struct ShellState<'a> {
    theme: &'a Theme,
    chapter_index: usize,
    active_section: usize,
    collapsed: bool,
    width: u16,
    height: u16,
    index_mode: IndexMode,
    content_width: u16,
    wide: bool,
    scroll_offset: ScrollOffset,
    query: String,
    results: Vec<SearchHit>,
    search_active: usize,
    preset: ThemePreset,
    mode: ThemeMode,
    navigator: Navigator,
    set_collapsed: &'a StateSetter<bool>,
    set_search_visible: &'a StateSetter<bool>,
    set_query: &'a StateSetter<String>,
    set_search_active: &'a StateSetter<usize>,
    search_active_ref: Arc<Mutex<usize>>,
    set_preset: &'a StateSetter<ThemePreset>,
    set_mode: &'a StateSetter<ThemeMode>,
}

/// The default application component.
fn app(_cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    shell.apply(ShellProps::default())
}

/// The shell component: header, body, and the search overlay.
fn shell(cx: &mut ComponentContext, props: &Props<ShellProps>) -> Node {
    let initial_preset = props.data().initial_preset;
    let initial_mode = props.data().initial_mode;
    let initial_chapter = props.data().initial_chapter;
    let initial_collapsed = props.data().initial_collapsed;
    let initial_search = props.data().initial_search;
    let initial_query = props.data().initial_query.clone();
    let (chapter_index, set_chapter) =
        cx.use_state(move || initial_chapter.min(metadata::CHAPTER_COUNT.saturating_sub(1)));
    let (active_section, set_active) = cx.use_state(|| 0usize);
    let (collapsed, set_collapsed) = cx.use_state(move || initial_collapsed);
    let (scroll_offset, set_scroll) = cx.use_state(ScrollOffset::default);
    let (width, set_width) = cx.use_state(|| ASSUMED_WIDTH);
    let (height, set_height) = cx.use_state(|| ASSUMED_HEIGHT);
    let (search_visible, set_search_visible) = cx.use_state(move || initial_search);
    let (query, set_query) = cx.use_state(move || initial_query);
    let (search_active, set_search_active) = cx.use_state(|| 0usize);
    let (preset, set_preset) = cx.use_state(move || initial_preset);
    let (mode, set_mode) = cx.use_state(move || initial_mode);
    let active_ref = cx.use_ref(|| 0usize);
    let pending = cx.use_ref(|| None::<usize>);
    let search_active_ref = cx.use_ref(|| 0usize);
    let viewport_ref = cx.use_element_ref();
    let root_ref = cx.use_element_ref();

    let section_refs = cx.use_memo(chapter_index, || {
        (0..metadata::section_count(chapter_index))
            .map(|_| ElementRef::new())
            .collect::<Vec<_>>()
    });
    let theme = cx.use_memo((preset, mode), move || {
        preset.theme(mode).customize(|theme| {
            theme.borders.kind = BorderKind::Rounded;
            theme.spacing.lg = 2;
            theme.spacing.xl = 3;
        })
    });

    let navigator = Navigator::new(
        chapter_index,
        section_refs,
        viewport_ref,
        NavBindings::new(set_chapter, set_active, set_scroll, active_ref, pending),
    );

    let index_mode = IndexMode::for_width(width, collapsed);
    let reserved = index_mode.reserved_width();
    let available = width.saturating_sub(reserved).max(12);
    let content_width = available;
    let wide = width >= demos::WIDE_BREAKPOINT;
    let results = if search_visible {
        search(&query)
    } else {
        Vec::new()
    };
    let clamped_active = search_active.min(results.len().saturating_sub(1));
    if clamped_active != search_active {
        set_search_active.set(clamped_active);
    }

    let state = ShellState {
        theme: &theme,
        chapter_index,
        active_section,
        collapsed,
        width,
        height,
        index_mode,
        content_width,
        wide,
        scroll_offset,
        query,
        results,
        search_active: clamped_active,
        preset,
        mode,
        navigator: navigator.clone(),
        set_collapsed: &set_collapsed,
        set_search_visible: &set_search_visible,
        set_query: &set_query,
        set_search_active: &set_search_active,
        search_active_ref: search_active_ref.clone(),
        set_preset: &set_preset,
        set_mode: &set_mode,
    };

    let keyboard_handler = {
        let navigator = navigator.clone();
        let chapter_index = state.chapter_index;
        let active_section = state.active_section;
        let search_open = search_visible;
        let section_count = state.navigator.section_count();
        let set_collapsed = set_collapsed.clone();
        let set_search_visible = set_search_visible.clone();
        let set_search_active = set_search_active.clone();
        let search_active_ref = search_active_ref.clone();
        move |event: KeyboardEvent| {
            let Some(shortcut) = shell_shortcut(&event) else {
                return;
            };
            match shortcut {
                Shortcut::ToggleSearch => {
                    if search_open {
                        set_search_visible.set(false);
                    } else {
                        if let Ok(mut active) = search_active_ref.lock() {
                            *active = 0;
                        }
                        set_search_active.set(0);
                        set_search_visible.set(true);
                    }
                }
                Shortcut::ToggleIndex => set_collapsed.update(|value| *value = !*value),
                Shortcut::PrevChapter => navigator.goto_chapter(
                    (chapter_index + metadata::CHAPTER_COUNT - 1) % metadata::CHAPTER_COUNT,
                ),
                Shortcut::NextChapter => {
                    navigator.goto_chapter((chapter_index + 1) % metadata::CHAPTER_COUNT)
                }
                Shortcut::PrevSection => navigator.goto_section(
                    active_section
                        .saturating_sub(1)
                        .min(section_count.saturating_sub(1)),
                ),
                Shortcut::NextSection => navigator
                    .goto_section((active_section + 1).min(section_count.saturating_sub(1))),
                Shortcut::CloseSearch => {
                    if search_open {
                        if let Ok(mut active) = search_active_ref.lock() {
                            *active = 0;
                        }
                        set_search_active.set(0);
                        set_search_visible.set(false);
                    }
                }
            }
        }
    };

    let metrics_handler = {
        let set_width = set_width.clone();
        let set_height = set_height.clone();
        move |snapshot: Option<ElementSnapshot>| {
            let Some(snapshot) = snapshot else {
                return;
            };
            let bounds = snapshot.bounding_rect();
            let measured = bounds.width.min(u32::from(u16::MAX)) as u16;
            let measured_height = bounds.height.min(u32::from(u16::MAX)) as u16;
            if measured > 0 {
                set_width.update(move |current| {
                    if *current != measured {
                        *current = measured;
                    }
                });
            }
            if measured_height > 0 {
                set_height.update(move |current| {
                    if *current != measured_height {
                        *current = measured_height;
                    }
                });
            }
        }
    };

    let header = header(&state);
    let sidebar = sidebar(&state);
    let body = document_column(&state);
    let overlay = if search_visible {
        search_overlay(&state)
    } else {
        empty()
    };

    let background = theme.colors.background;
    let foreground = theme.colors.foreground;
    let theme_for_provider = theme.clone();
    let root = ui! {
        <view
            element_ref={root_ref}
            on_element_change={metrics_handler}
            on_app_key={keyboard_handler}
            style={move |style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Percent(Percent::viewport(100));
                style.height /= Dimension::Percent(Percent::viewport(100));
                style.gap /= 0;
                style.background /= background;
                style.text.foreground /= foreground;
            }}
        >
            {header}
            <view style={|style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.height /= Dimension::Max;
                style.gap /= 0;
            }}>
                {sidebar}
                <view style={|style| {
                    // Absolute here means "children may be placed", which is how
                    // the search dialog floats above the document instead of
                    // taking rows away from it.
                    style.layout /= Layout::Absolute;
                    style.width /= Dimension::Max;
                    style.height /= Dimension::Max;
                    style.gap /= 0;
                }}>
                    {body}
                    {overlay}
                </view>
            </view>
        </view>
    };
    ui! {
        <theme_provider value={theme_for_provider}>
            {root}
        </theme_provider>
    }
}

/// Brand, breadcrumb, chapter navigation, and theme controls.
fn header(state: &ShellState<'_>) -> Node {
    let theme = state.theme;
    let meta = metadata::chapter(state.chapter_index);
    let card = theme.colors.card;
    let card_foreground = theme.colors.card_foreground;
    let border = theme.colors.border;
    let primary = theme.colors.primary;
    let secondary = theme.colors.secondary;
    let muted_foreground = theme.colors.muted_foreground;

    let navigator_back = state.navigator.clone();
    let navigator_forward = state.navigator.clone();
    let navigator_section_back = state.navigator.clone();
    let navigator_section_forward = state.navigator.clone();
    let chapter_index = state.chapter_index;
    let active_section = state.active_section;
    let section_count = state.navigator.section_count();
    let set_collapsed = state.set_collapsed.clone();
    let set_search_visible = state.set_search_visible.clone();
    let set_search_active = state.set_search_active.clone();
    let search_active_ref = state.search_active_ref.clone();
    let set_preset = state.set_preset.clone();
    let set_mode = state.set_mode.clone();
    let preset = state.preset;
    let mode = state.mode;
    let index_mode = state.index_mode;
    let collapsed = state.collapsed;
    let show_verbose = state.wide;
    // Below the narrow breakpoint the navigation cluster and the title cannot
    // share a row, and a title cut mid-word reads as a broken bar. The title
    // wins the row and wraps instead.
    let show_navigation = state.width >= demos::NARROW_BREAKPOINT;
    // The brand and the navigation cluster give up their cells below the narrow
    // breakpoint, because the chapter title is the one thing the bar must always
    // say in full. It then fits on one line instead of being cut mid-word.
    let brand = if show_navigation {
        Node::from(Text::from_spans([
            Span::new("◆").foreground(primary).bold(),
            Span::new(" ICMD FIELD GUIDE")
                .foreground(card_foreground)
                .bold(),
        ]))
    } else {
        empty()
    };

    let index_label = if index_mode == IndexMode::Narrow {
        String::from("index hidden")
    } else if index_mode == IndexMode::Wide {
        String::from("← index")
    } else {
        String::from("→ index")
    };
    let mode_glyph = match mode {
        ThemeMode::Dark => "☾",
        ThemeMode::Light => "☀",
    };
    let mode_label = match mode {
        ThemeMode::Dark => "dark",
        ThemeMode::Light => "light",
    };
    let preset_name = preset.name();

    ui! {
        <view style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.padding /= Edges::symmetric(0, 1);
            style.background /= card;
            style.text.foreground /= card_foreground;
            style.border.kind /= BorderKind::Single;
            style.border.edges /= Edges { top: false, right: false, bottom: true, left: false };
            style.border.foreground /= border;
        }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
                style.gap /= 1;
            }}>
                <row style={|style| { style.align /= Align::Center; style.gap /= 1; }}>
                    {brand}
                    {Text::new(format!("/ {}", breadcrumb(meta))).foreground(secondary)}
                </row>
                {if show_navigation {
                    ui! {
                <row style={|style| { style.align /= Align::Center; style.gap /= 1; }}>
                    <button variant={ButtonVariant::Secondary}
                        on_press={move |_| navigator_back.goto_chapter(
                            (chapter_index + metadata::CHAPTER_COUNT - 1) % metadata::CHAPTER_COUNT
                        )}
                        style={move |style| {
                            style.padding /= Edges::symmetric(0, 1);
                            style.background /= card;
                            style.text.foreground /= muted_foreground;
                            style.border.edges /= Edges::all(false);
                        }}>"‹ prev"</button>
                    <button variant={ButtonVariant::Secondary}
                        on_press={move |_| navigator_forward.goto_chapter(
                            (chapter_index + 1) % metadata::CHAPTER_COUNT
                        )}
                        style={move |style| {
                            style.padding /= Edges::symmetric(0, 1);
                            style.background /= card;
                            style.text.foreground /= muted_foreground;
                            style.border.edges /= Edges::all(false);
                        }}>"next ›"</button>
                    <button variant={ButtonVariant::Secondary}
                        on_press={move |_| navigator_section_back.goto_section(active_section.saturating_sub(1))}
                        disabled={section_count == 0}
                        style={move |style| {
                            style.padding /= Edges::symmetric(0, 1);
                            style.background /= card;
                            style.text.foreground /= muted_foreground;
                            style.border.edges /= Edges::all(false);
                        }}>"§ prev"</button>
                    <button variant={ButtonVariant::Secondary}
                        on_press={move |_| navigator_section_forward.goto_section((active_section + 1).min(section_count.saturating_sub(1)))}
                        disabled={section_count == 0}
                        style={move |style| {
                            style.padding /= Edges::symmetric(0, 1);
                            style.background /= card;
                            style.text.foreground /= muted_foreground;
                            style.border.edges /= Edges::all(false);
                        }}>"§ next"</button>
                    <button variant={ButtonVariant::Secondary}
                        on_press={move |_| {
                            if let Ok(mut active) = search_active_ref.lock() { *active = 0; }
                            set_search_active.set(0);
                            set_search_visible.set(true);
                        }}
                        style={move |style| {
                            style.padding /= Edges::symmetric(0, 1);
                            style.background /= card;
                            style.text.foreground /= primary;
                            style.border.edges /= Edges::all(false);
                        }}>"⌕ search"</button>
                    <button variant={ButtonVariant::Secondary}
                        on_press={move |_| set_collapsed.update(|value| *value = !*value)}
                        disabled={index_mode == IndexMode::Narrow}
                        style={move |style| {
                            style.padding /= Edges::symmetric(0, 1);
                            style.background /= card;
                            style.text.foreground /= muted_foreground;
                            style.border.edges /= Edges::all(false);
                        }}>{index_label}</button>
                    <button variant={ButtonVariant::Secondary}
                        on_press={move |_| set_preset.set(next_preset(preset))}
                        style={move |style| {
                            style.padding /= Edges::symmetric(0, 1);
                            style.background /= card;
                            style.text.foreground /= secondary;
                            style.border.edges /= Edges::all(false);
                        }}>{format!("◐ {preset_name}")}</button>
                    <button variant={ButtonVariant::Secondary}
                        on_press={move |_| set_mode.set(next_mode(mode))}
                        style={move |style| {
                            style.padding /= Edges::symmetric(0, 1);
                            style.background /= card;
                            style.text.foreground /= secondary;
                            style.border.edges /= Edges::all(false);
                        }}>{format!("{mode_glyph} {mode_label}")}</button>
                </row>
                    }
                } else {
                    empty()
                }}
            </row>
            {if show_verbose {
                ui! {
                    <row style={|style| {
                        style.width /= Dimension::Max;
                        style.align /= Align::Center;
                        style.gap /= 1;
                    }}>
                        <muted>"a terminal-native field guide"</muted>
                        <kbd>"ctrl+k"</kbd><muted>"search"</muted>
                        <kbd>"ctrl+b"</kbd><muted>"index"</muted>
                        <kbd>"alt+←/→"</kbd><muted>"chapter"</muted>
                        <kbd>"alt+↑/↓"</kbd><muted>"section"</muted>
                        <kbd>"ctrl+c"</kbd><muted>"exit"</muted>
                    </row>
                }
            } else {
                ui! {
                    <row style={|style| {
                        style.width /= Dimension::Max;
                        style.align /= Align::Center;
                        style.gap /= 1;
                    }}>
                        <kbd>"ctrl+k"</kbd><muted>"search"</muted>
                        <kbd>"ctrl+b"</kbd><muted>"index"</muted>
                    </row>
                }
            }}
            {if collapsed && index_mode != IndexMode::Wide {
                ui! { <muted>{format!("index collapsed · ctrl+b to expand · {} chapters", metadata::CHAPTER_COUNT)}</muted> }
            } else {
                empty()
            }}
        </view>
    }
}

/// The document column that holds the mounted chapter.
///
/// The column takes every cell the index leaves, so prose, code, and
/// demonstrations all use the full width of the terminal instead of a narrow
/// centered strip.
fn document_column(state: &ShellState<'_>) -> Node {
    let props = ChapterProps::new(state.navigator.clone(), state.scroll_offset, state.wide);
    let chapter_index = state.navigator.chapter_index();
    let content = selected_chapter(chapter_index, props);
    let background = state.theme.colors.background;
    let chapter_key = metadata::chapter(chapter_index).id.to_string();
    ui! {
        <view key={chapter_key} style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.height /= Dimension::Max;
            style.align /= Align::Stretch;
            style.gap /= 0;
            style.background /= background;
        }}>
            {content}
        </view>
    }
}

/// Chooses and renders the index treatment for the current width.
fn sidebar(state: &ShellState<'_>) -> Node {
    match state.index_mode {
        IndexMode::Wide => expanded_index(state),
        IndexMode::Mixed => compact_rail(state),
        IndexMode::Narrow => empty(),
    }
}

/// Full editorial index with chapter and section entries.
fn expanded_index(state: &ShellState<'_>) -> Node {
    let theme = state.theme;
    let card = theme.colors.card;
    let card_foreground = theme.colors.card_foreground;
    let border = theme.colors.border;
    let primary = theme.colors.primary;
    let muted_foreground = theme.colors.muted_foreground;

    let chapters = (0..metadata::CHAPTER_COUNT)
        .map(|index| index_chapter_item(state, index))
        .collect::<Node>();
    let sections = (0..state.navigator.section_count())
        .map(|index| index_section_item(state, index))
        .collect::<Node>();
    let set_collapsed = state.set_collapsed.clone();
    let title = metadata::chapter(state.chapter_index).title;

    ui! {
        <view style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Cells(demos::INDEX_WIDTH);
            style.height /= Dimension::Max;
            style.gap /= 0;
            style.padding /= Edges::all(1);
            style.background /= card;
            style.text.foreground /= card_foreground;
            style.border.kind /= BorderKind::Single;
            style.border.edges /= Edges { top: false, right: true, bottom: false, left: false };
            style.border.foreground /= border;
        }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
                style.gap /= 0;
            }}>
                {Text::new("INDEX").foreground(primary).bold()}
                <button variant={ButtonVariant::Secondary} on_press={move |_| set_collapsed.set(true)}
                    style={move |style| {
                        style.padding /= Edges::symmetric(0, 1);
                        style.background /= card;
                        style.text.foreground /= muted_foreground;
                        style.border.edges /= Edges::all(false);
                    }}>"←"</button>
            </row>
            <scroll_area axes={ScrollAxes::Vertical}
                scrollbar_visibility={ScrollbarVisibility::Auto}
                wheel_step={2_u16}
                style={|style| {
                    style.layout /= Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.height /= Dimension::Max;
                    style.gap /= 0;
                }}>
                <view style={|style| {
                    style.layout /= Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.gap /= 0;
                }}>
                    <muted>{Text::new("CHAPTERS").bold()}</muted>
                    {chapters}
                    <divider />
                    <row style={|style| {
                        style.width /= Dimension::Max;
                        style.justify /= Justify::SpaceBetween;
                        style.gap /= 0;
                    }}>
                        <muted>{Text::new("ON THIS PAGE").bold()}</muted>
                        <muted>{format!("{}/{}", state.active_section + 1, state.navigator.section_count())}</muted>
                    </row>
                    <muted>{Text::new(title).wrap(TextWrap::Soft)}</muted>
                    {sections}
                </view>
            </scroll_area>
            <divider />
            <muted>{Text::new("ctrl+b collapse · ctrl+k search").wrap(TextWrap::Soft)}</muted>
        </view>
    }
}

/// One chapter entry in the expanded index.
fn index_chapter_item(state: &ShellState<'_>, index: usize) -> Node {
    let theme = state.theme;
    let meta = metadata::chapter(index);
    let selected = index == state.chapter_index;
    let (background, foreground) = if selected {
        (theme.colors.primary, theme.colors.primary_foreground)
    } else {
        (theme.colors.card, theme.colors.card_foreground)
    };
    let number_color = if selected {
        theme.colors.primary_foreground
    } else {
        theme.colors.muted_foreground
    };
    let navigator = state.navigator.clone();
    let number = meta.number;
    let title = meta.title;
    ui! {
        <button key={index as u64} variant={ButtonVariant::Secondary}
            on_press={move |_| navigator.goto_chapter(index)}
            style={move |style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.gap /= 1;
                style.padding /= Edges::symmetric(0, 1);
                style.background /= background;
                style.text.foreground /= foreground;
                style.text.attr.bold /= selected;
                style.border.edges /= Edges::all(false);
            }}>
            {Text::new(number).foreground(number_color)}
            <view style={|style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 0;
            }}>
                {Text::new(title).wrap(TextWrap::Soft)}
            </view>
        </button>
    }
}

/// One section entry in the expanded index.
fn index_section_item(state: &ShellState<'_>, index: usize) -> Node {
    let theme = state.theme;
    let card = theme.colors.card;
    let muted_foreground = theme.colors.muted_foreground;
    let meta = &metadata::chapter(state.chapter_index).sections[index];
    let selected = index == state.active_section;
    let marker = if selected { "◆" } else { "│" };
    let foreground = if selected {
        theme.colors.primary
    } else {
        muted_foreground
    };
    let navigator = state.navigator.clone();
    let number = meta.number;
    let title = meta.title;
    ui! {
        <button key={index as u64} variant={ButtonVariant::Secondary}
            on_press={move |_| navigator.goto_section(index)}
            style={move |style| {
                style.layout /= Layout::Horizontal;
                style.width /= Dimension::Max;
                style.gap /= 1;
                style.padding /= Edges { top: 0, right: 1, bottom: 0, left: 1 };
                style.background /= card;
                style.text.foreground /= foreground;
                style.text.attr.bold /= selected;
                style.border.edges /= Edges::all(false);
            }}>
            {Text::new(marker).foreground(foreground)}
            {Text::new(number).foreground(muted_foreground)}
            <view style={|style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 0;
            }}>
                {Text::new(title).wrap(TextWrap::Soft)}
            </view>
        </button>
    }
}

/// Compact numbered rail with section dots.
fn compact_rail(state: &ShellState<'_>) -> Node {
    let theme = state.theme;
    let card = theme.colors.card;
    let border = theme.colors.border;
    let primary = theme.colors.primary;
    let set_collapsed = state.set_collapsed.clone();

    let chapters = (0..metadata::CHAPTER_COUNT)
        .map(|index| rail_chapter_item(state, index))
        .collect::<Node>();
    let dots = (0..state.navigator.section_count())
        .map(|index| rail_section_item(state, index))
        .collect::<Node>();

    ui! {
        <view style={move |style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Cells(demos::RAIL_WIDTH);
            style.height /= Dimension::Max;
            style.gap /= 0;
            style.padding /= Edges::symmetric(0, 1);
            style.align /= Align::Center;
            style.background /= card;
            style.border.kind /= BorderKind::Single;
            style.border.edges /= Edges { top: false, right: true, bottom: false, left: false };
            style.border.foreground /= border;
        }}>
            <button variant={ButtonVariant::Secondary} on_press={move |_| set_collapsed.set(false)}
                style={move |style| {
                    style.width /= Dimension::Max;
                    style.padding /= Edges::all(0);
                    style.background /= card;
                    style.text.foreground /= primary;
                    style.border.edges /= Edges::all(false);
                }}>
                "→"
            </button>
            <scroll_area axes={ScrollAxes::Vertical}
                scrollbar_visibility={ScrollbarVisibility::Auto}
                wheel_step={2_u16}
                style={|style| {
                    style.layout /= Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.height /= Dimension::Max;
                    style.gap /= 0;
                    style.align /= Align::Center;
                }}>
                <view style={|style| {
                    style.layout /= Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.gap /= 0;
                    style.align /= Align::Center;
                }}>
                    {chapters}
                    <divider />
                    {dots}
                </view>
            </scroll_area>
        </view>
    }
}

/// One numbered chapter button in the compact rail.
fn rail_chapter_item(state: &ShellState<'_>, index: usize) -> Node {
    let theme = state.theme;
    let selected = index == state.chapter_index;
    let number = metadata::chapter(index).number;
    let navigator = state.navigator.clone();
    let background = if selected {
        theme.colors.primary
    } else {
        theme.colors.card
    };
    let foreground = if selected {
        theme.colors.primary_foreground
    } else {
        theme.colors.muted_foreground
    };
    ui! {
        <button key={index as u64} variant={ButtonVariant::Secondary}
            on_press={move |_| navigator.goto_chapter(index)}
            style={move |style| {
                style.width /= Dimension::Max;
                style.padding /= Edges::all(0);
                style.background /= background;
                style.text.foreground /= foreground;
                style.border.edges /= Edges::all(false);
            }}>
            {Text::new(number)}
        </button>
    }
}

/// One section dot in the compact rail.
fn rail_section_item(state: &ShellState<'_>, index: usize) -> Node {
    let theme = state.theme;
    let card = theme.colors.card;
    let border = theme.colors.border;
    let selected = index == state.active_section;
    let navigator = state.navigator.clone();
    let glyph = if selected { "◆" } else { "·" };
    let foreground = if selected {
        theme.colors.primary
    } else {
        border
    };
    ui! {
        <button key={index as u64} variant={ButtonVariant::Secondary}
            on_press={move |_| navigator.goto_section(index)}
            style={move |style| {
                style.width /= Dimension::Max;
                style.padding /= Edges::all(0);
                style.background /= card;
                style.text.foreground /= foreground;
                style.border.edges /= Edges::all(false);
            }}>
            {Text::new(glyph)}
        </button>
    }
}

/// The `Ctrl+K` search overlay: input, ranked results, and an empty state.
fn search_overlay(state: &ShellState<'_>) -> Node {
    let theme = state.theme;
    let card = theme.colors.card;
    let card_foreground = theme.colors.card_foreground;
    let primary = theme.colors.primary;
    let muted_foreground = theme.colors.muted_foreground;
    let width = state
        .content_width
        .saturating_sub(4)
        .clamp(34, MAX_SEARCH_WIDTH)
        .min(state.width.saturating_sub(2).max(20));
    // The dialog sits a couple of rows below the header and grows only as far as
    // its results need, never as far as the viewport allows: an unbounded card
    // covered the whole page and read as an opaque sheet rather than a card.
    let height = state.height.saturating_sub(8).clamp(7, MAX_SEARCH_HEIGHT);

    let query = state.query.clone();
    let set_query = state.set_query.clone();
    let set_search_visible = state.set_search_visible.clone();
    let set_search_active = state.set_search_active.clone();
    let search_active_ref = state.search_active_ref.clone();
    let results = state.results.clone();
    let active = state.search_active;
    let navigator = state.navigator.clone();
    let set_search_visible_for_keys = set_search_visible.clone();
    let set_search_active_for_keys = set_search_active.clone();
    let search_active_ref_for_keys = search_active_ref.clone();
    let results_for_keys = results.clone();
    let locations = results.len();

    let key_handler = move |event: KeyboardEvent| match event.key.code {
        KeyCode::Esc => {
            if let Ok(mut active) = search_active_ref_for_keys.lock() {
                *active = 0;
            }
            set_search_active_for_keys.set(0);
            set_search_visible_for_keys.set(false);
        }
        KeyCode::Up => {
            event.stop_propagation();
            event.prevent_default();
            move_search_selection(
                &results_for_keys,
                &search_active_ref_for_keys,
                &set_search_active_for_keys,
                false,
            );
        }
        KeyCode::Down => {
            event.stop_propagation();
            event.prevent_default();
            move_search_selection(
                &results_for_keys,
                &search_active_ref_for_keys,
                &set_search_active_for_keys,
                true,
            );
        }
        KeyCode::Enter => {
            event.stop_propagation();
            activate_result(
                &results_for_keys,
                &search_active_ref_for_keys,
                &navigator,
                &set_search_visible_for_keys,
                &set_search_active_for_keys,
            );
        }
        _ => {}
    };

    let rows = results
        .iter()
        .enumerate()
        .map(|(index, hit)| search_result_row(state, hit, index == active))
        .collect::<Node>();

    let result_area = if state.query.trim().is_empty() {
        ui! {
            <muted>"Type a chapter, widget, hook, type, or alias. Matching is exact, then prefix, then substring."</muted>
        }
    } else if results.is_empty() {
        ui! {
            <view style={|style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 0;
            }}>
                {Text::new(format!("No match for \"{}\"", state.query.trim())).foreground(muted_foreground)}
                <muted>"Try a component name such as `scroll_area`, or an alias such as `layout`."</muted>
            </view>
        }
    } else {
        ui! { {rows} }
    };

    ui! {
        <view style={move |style| {
            // Absolute keeps the card above the document instead of in its flow:
            // in the flow it squeezed the chapter into whatever rows were left
            // and read as a sheet covering the page.
            style.line /= AxisPosition::Cells(2);
            style.column /= AxisPosition::Center;
            style.width /= Dimension::Cells(width);
            style.height /= Dimension::Cells(height);
            style.z_index /= 20;
            style.gap /= 1;
            style.padding /= Edges { top: 0, right: 1, bottom: 1, left: 1 };
            style.background /= card;
            style.text.foreground /= card_foreground;
            style.border.kind /= BorderKind::Rounded;
            style.border.foreground /= primary;
            style.border.background /= card;
        }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
                style.gap /= 1;
            }}>
                {Text::new("SEARCH").foreground(primary).bold()}
                <muted>"↑↓ move · enter open · esc close"</muted>
            </row>
            <input value={query} autofocus
                placeholder={"Search chapters, widgets, hooks, and types"}
                on_change={move |event: icmd::TextValueEvent| set_query.set(event.value)}
                on_key_down={key_handler}
                style={|style| {
                    style.width /= Dimension::Max;
                    style.height /= Dimension::Cells(1);
                    style.border.edges /= Edges::all(false);
                }} />
            <scroll_area axes={ScrollAxes::Vertical}
                scrollbar_visibility={ScrollbarVisibility::Auto}
                wheel_step={2_u16}
                style={|style| {
                    style.layout /= Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.height /= Dimension::Max;
                    style.gap /= 0;
                }}>
                {result_area}
            </scroll_area>
            <muted>{format!("{locations} result(s) · covers every shipped widget and documented hook")}</muted>
        </view>
    }
}

/// Moves the highlighted search result with wraparound.
fn move_search_selection(
    results: &[SearchHit],
    active_ref: &Arc<Mutex<usize>>,
    set_active: &StateSetter<usize>,
    forward: bool,
) {
    let count = results.len();
    if count == 0 {
        return;
    }
    let current = active_ref
        .lock()
        .map(|active| *active)
        .unwrap_or(0)
        .min(count - 1);
    let next = if forward {
        (current + 1) % count
    } else {
        (current + count - 1) % count
    };
    if let Ok(mut active) = active_ref.lock() {
        *active = next;
    }
    set_active.set(next);
}

/// Navigates to the highlighted result and closes the overlay.
fn activate_result(
    results: &[SearchHit],
    active_ref: &Arc<Mutex<usize>>,
    navigator: &Navigator,
    set_visible: &StateSetter<bool>,
    set_active: &StateSetter<usize>,
) {
    let index = active_ref.lock().map(|active| *active).unwrap_or(0);
    let Some(hit) = results.get(index).cloned() else {
        return;
    };
    if let Ok(mut active) = active_ref.lock() {
        *active = 0;
    }
    set_active.set(0);
    set_visible.set(false);
    if hit.kind == HitKind::Chapter {
        navigator.goto_chapter(hit.chapter);
    } else {
        navigator.goto(hit.chapter, hit.section);
    }
}

/// One clickable search result row.
fn search_result_row(state: &ShellState<'_>, hit: &SearchHit, selected: bool) -> Node {
    let theme = state.theme;
    let primary_foreground = theme.colors.primary_foreground;
    let card = theme.colors.card;
    let card_foreground = theme.colors.card_foreground;
    let muted_foreground = theme.colors.muted_foreground;
    let background = if selected { theme.colors.primary } else { card };
    let label_color = if selected {
        primary_foreground
    } else {
        card_foreground
    };
    let detail_color = if selected {
        primary_foreground
    } else {
        muted_foreground
    };
    let kind = hit.kind.label();
    let label = hit.label.clone();
    let location = hit.location.clone();
    let navigator = state.navigator.clone();
    let chapter = hit.chapter;
    let section = hit.section;
    let chapter_hit = hit.kind == HitKind::Chapter;
    let set_search_visible = state.set_search_visible.clone();
    let set_search_active = state.set_search_active.clone();
    let search_active_ref = state.search_active_ref.clone();
    ui! {
        <button variant={ButtonVariant::Secondary}
            on_press={move |_| {
                if let Ok(mut active) = search_active_ref.lock() { *active = 0; }
                set_search_active.set(0);
                set_search_visible.set(false);
                if chapter_hit { navigator.goto_chapter(chapter); } else { navigator.goto(chapter, section); }
            }}
            style={move |style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 0;
                style.padding /= Edges { top: 0, right: 1, bottom: 0, left: 1 };
                style.background /= background;
                style.text.foreground /= label_color;
                style.border.edges /= Edges::all(false);
            }}>
            <row style={|style| { style.width /= Dimension::Max; style.align /= Align::Center; style.gap /= 1; }}>
                {Text::new(format!("{kind:<8}")).foreground(detail_color)}
                {Text::new(label).foreground(label_color).bold().overflow(TextOverflow::Ellipsis)}
            </row>
            <row style={|style| { style.width /= Dimension::Max; style.gap /= 1; }}>
                {Text::new(location).foreground(detail_color).align(TextAlign::End).overflow(TextOverflow::Ellipsis)}
            </row>
        </button>
    }
}

/// Mounts the shell for tests and diagnostics.
#[cfg(test)]
pub(crate) fn mount(props: ShellProps) -> Node {
    shell.apply(props)
}

/// Entry point for `cargo icmd docs`.
pub(crate) fn run() -> Result<(), icmd::RenderError> {
    render(app.apply(()), RuntimeConfig::default())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crossterm::event::KeyEvent;
    use icmd::advanced::{Commit, Lower, Runtime, ShutdownPolicy};
    use icmd::{Frame, Operation, Size};

    use super::*;

    /// Commits one shell frame and returns it, shutting the pipeline down.
    fn commit_shell(props: ShellProps, viewport: Size) -> Frame {
        let (commit, _, _) = Commit::new_with_events(viewport);
        let runtime = Runtime::new(Lower::default()).then(commit).start_handle();
        let input = runtime.input();
        let output = runtime.output();
        input
            .send(shell.apply(props))
            .expect("shell enters the pipeline");
        let frame = output
            .recv_timeout(Duration::from_secs(5))
            .expect("shell commits a frame");
        drop(input);
        runtime.shutdown(ShutdownPolicy::default()).unwrap();
        frame
    }

    /// Commits one frame and asserts it painted.
    fn assert_commits(props: ShellProps, viewport: Size) -> Frame {
        let frame = commit_shell(props, viewport);
        assert!(
            !frame.operations.is_empty(),
            "shell must paint a non-empty frame at {viewport:?}"
        );
        frame
    }

    /// Counts the cell surfaces a frame creates.
    fn created_surface_widths(frame: &Frame) -> Vec<u32> {
        frame
            .operations
            .iter()
            .filter_map(|operation| match operation {
                Operation::Create { image, .. } => Some(image.width() as u32),
                _ => None,
            })
            .collect()
    }

    /// Every chapter commits at every supported viewport in both themes.
    ///
    /// This exercises all three sidebar modes: 140 cells selects the expanded
    /// index, 120 and 80 select the compact rail, and 60 hides it.
    #[test]
    fn commits_every_chapter_at_every_supported_viewport_in_both_themes() {
        for chapter in 0..metadata::CHAPTER_COUNT {
            for mode in [ThemeMode::Dark, ThemeMode::Light] {
                for viewport in [
                    Size::new(140, 45),
                    Size::new(120, 40),
                    Size::new(80, 24),
                    Size::new(60, 20),
                ] {
                    assert_commits(
                        ShellProps {
                            initial_chapter: chapter,
                            initial_mode: mode,
                            ..ShellProps::default()
                        },
                        viewport,
                    );
                }
            }
        }
    }

    /// A collapsed index still commits at every width, including where the
    /// expanded index cannot fit.
    /// A query must paint ranked result rows, not just the empty prompt.
    ///
    /// The commit emits one surface per styled text run, so a longer result
    /// list shows up as more operations: the prompt paints a single hint line,
    /// while eight ranked rows paint a kind, a label, and a location each.
    #[test]
    fn search_overlay_paints_ranked_results_for_a_query() {
        let viewport = Size::new(120, 40);
        let prompt = assert_commits(
            ShellProps {
                initial_search: true,
                ..ShellProps::default()
            },
            viewport,
        );
        let results = assert_commits(
            ShellProps {
                initial_search: true,
                initial_query: String::from("scroll"),
                ..ShellProps::default()
            },
            viewport,
        );
        assert!(
            search("scroll").len() > 1,
            "the query used by this test must produce several results"
        );
        assert!(
            results.operations.len() > prompt.operations.len(),
            "a query must paint result rows ({} vs {} operations)",
            results.operations.len(),
            prompt.operations.len()
        );
    }

    #[test]
    fn collapsed_index_commits_at_every_sidebar_mode() {
        for viewport in [Size::new(140, 45), Size::new(100, 30), Size::new(60, 20)] {
            assert_commits(
                ShellProps {
                    initial_collapsed: true,
                    ..ShellProps::default()
                },
                viewport,
            );
        }
    }

    /// The search overlay adds a positioned surface above the document.
    #[test]
    fn search_overlay_commits_and_adds_a_positioned_surface() {
        let viewport = Size::new(120, 40);
        let closed = assert_commits(ShellProps::default(), viewport);
        let open = assert_commits(
            ShellProps {
                initial_search: true,
                ..ShellProps::default()
            },
            viewport,
        );
        let closed_surfaces = created_surface_widths(&closed).len();
        let open_surfaces = created_surface_widths(&open).len();
        assert!(
            open_surfaces > closed_surfaces,
            "the overlay must add its own surface ({open_surfaces} vs {closed_surfaces})"
        );
        let viewport_width = viewport.width as u32;
        assert!(
            created_surface_widths(&open)
                .iter()
                .any(|width| *width < viewport_width && *width > 20),
            "the overlay must be sized inside the viewport"
        );
    }

    #[test]
    fn index_mode_respects_width_and_reader_preference() {
        assert_eq!(IndexMode::for_width(140, false), IndexMode::Wide);
        assert_eq!(IndexMode::for_width(140, true), IndexMode::Mixed);
        assert_eq!(IndexMode::for_width(110, false), IndexMode::Wide);
        assert_eq!(IndexMode::for_width(109, false), IndexMode::Mixed);
        assert_eq!(IndexMode::for_width(72, false), IndexMode::Mixed);
        assert_eq!(IndexMode::for_width(71, false), IndexMode::Narrow);
        assert_eq!(IndexMode::for_width(60, true), IndexMode::Narrow);
    }

    #[test]
    fn shortcut_only_matches_modifier_chords() {
        for code in [
            KeyCode::Char('q'),
            KeyCode::Char('1'),
            KeyCode::Char('k'),
            KeyCode::Char('b'),
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Up,
            KeyCode::Down,
        ] {
            let event = KeyboardEvent {
                key: KeyEvent::new(code, KeyModifiers::NONE),
            };
            assert_eq!(
                shell_shortcut(&event),
                None,
                "bare {code:?} must not be a shell shortcut"
            );
        }
        let cases = [
            (
                KeyCode::Char('k'),
                KeyModifiers::CONTROL,
                Shortcut::ToggleSearch,
            ),
            (
                KeyCode::Char('b'),
                KeyModifiers::CONTROL,
                Shortcut::ToggleIndex,
            ),
            (KeyCode::Left, KeyModifiers::ALT, Shortcut::PrevChapter),
            (KeyCode::Right, KeyModifiers::ALT, Shortcut::NextChapter),
            (KeyCode::Up, KeyModifiers::ALT, Shortcut::PrevSection),
            (KeyCode::Down, KeyModifiers::ALT, Shortcut::NextSection),
            (KeyCode::Esc, KeyModifiers::NONE, Shortcut::CloseSearch),
        ];
        for (code, modifiers, expected) in cases {
            let event = KeyboardEvent {
                key: KeyEvent::new(code, modifiers),
            };
            assert_eq!(shell_shortcut(&event), Some(expected), "{code:?}");
        }
    }

    #[test]
    fn the_guide_prefers_a_detected_native_image_protocol() {
        // The tool requires the native-raster build but never forces a protocol:
        // Auto prefers Kitty, Sixel, or iTerm2 where the terminal supports one
        // and falls back to symbol rendering everywhere else.
        assert_eq!(
            RuntimeConfig::default().image_protocol,
            icmd::ImageProtocol::Auto
        );
    }

    #[test]
    fn preset_and_mode_are_independent_controls() {
        // Cycling the preset must not move the mode, and toggling the mode must
        // not move the preset: the header exposes them as separate state.
        let mut preset = ThemePreset::Solarized;
        let mut mode = ThemeMode::Dark;
        for _ in 0..ThemePreset::ALL.len() {
            preset = next_preset(preset);
            assert_eq!(
                mode,
                ThemeMode::Dark,
                "preset cycling must not change the mode"
            );
        }
        mode = next_mode(mode);
        assert_eq!(mode, ThemeMode::Light);
        assert_eq!(
            preset,
            ThemePreset::Solarized,
            "the preset must survive a mode toggle"
        );
        mode = next_mode(mode);
        assert_eq!(mode, ThemeMode::Dark);
        assert_eq!(preset, ThemePreset::Solarized);
    }

    #[test]
    fn preset_cycling_covers_every_preset_and_mode_toggles() {
        let mut preset = ThemePreset::Nord;
        let mut seen = vec![preset];
        for _ in 0..ThemePreset::ALL.len() {
            preset = next_preset(preset);
            seen.push(preset);
        }
        for expected in ThemePreset::ALL {
            assert!(seen.contains(&expected), "cycling missed {expected:?}");
        }
        assert_eq!(next_mode(ThemeMode::Dark), ThemeMode::Light);
        assert_eq!(next_mode(ThemeMode::Light), ThemeMode::Dark);
    }
}
