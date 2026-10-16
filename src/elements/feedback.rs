//! Status feedback: progress bars, spinners, badges, alerts, and skeletons.

use crate::{
    Attr, Dimension, DomProps, Edges, EventListener, Glyph, GlyphRamp, Layout, Node, PointerEvent,
    Props, Span, Text,
    basic::{ComponentContext, view},
    ui,
};
/// Configuration for [`progress_bar`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProgressBarProps {
    /// Completed units; defaults to `0` and is clamped to `max`.
    pub value: Attr<u64>,
    /// Total units; defaults to `100` and is floored at `1`.
    pub max: Attr<u64>,
    /// Bar width in terminal cells; defaults to `20`.
    pub width: Attr<u16>,
    /// Whether the trailing percentage is shown; defaults to `true`.
    pub show_percentage: Attr<bool>,
    /// Optional text drawn before the bar; defaults to empty.
    pub label: Attr<String>,
    /// Glyph for a fully filled cell; defaults to the theme's progress style.
    pub filled: Attr<Glyph>,
    /// Glyph for an empty cell; defaults to the theme's progress style.
    pub empty: Attr<Glyph>,
    /// Ordered glyphs for the fractional cell, from the least to the most
    /// filled; defaults to the theme's progress style. An empty sequence
    /// disables the fractional cell and rounds the filled count instead.
    pub partial: Attr<GlyphRamp>,
    /// Whether the bar answers pointer drags; defaults to `false`.
    ///
    /// A draggable bar stays caller-owned: it reports a proposed value through
    /// [`ProgressBarProps::on_drag_start`], [`ProgressBarProps::on_drag`], and
    /// [`ProgressBarProps::on_drag_end`] and never changes the value it renders.
    pub draggable: Attr<bool>,
    /// Listener for the value under the press that starts a drag.
    pub on_drag_start: Attr<EventListener<ProgressBarDragEvent>>,
    /// Listener for each distinct value while a drag is in progress.
    pub on_drag: Attr<EventListener<ProgressBarDragEvent>>,
    /// Listener for the final value when a drag ends.
    pub on_drag_end: Attr<EventListener<ProgressBarDragEvent>>,
}

/// A value proposed by dragging a draggable [`progress_bar`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProgressBarDragEvent {
    /// Value under the pointer, clamped to `0..=max`.
    pub value: u64,
    /// Maximum the bar was rendered with.
    pub max: u64,
}

/// Interaction state a draggable bar keeps across frames: whether a drag is in
/// progress and the value last reported, so an unchanged value is reported
/// once. The rendered value stays the caller's.
#[derive(Default)]
struct ProgressDragState {
    /// Whether a pointer drag is in progress.
    dragging: bool,
    /// The value most recently reported to the caller.
    value: u64,
}

/// Horizontal progress bar filled in proportion to `value / max`; see
/// [`ProgressBarProps`] for its configuration.
///
/// The bar is one horizontal row: an optional label, the bar itself, and an
/// optional percentage. Only the bar is a drag target, so pressing the label or
/// the percentage never starts a drag. The bar's cells are split into full
/// cells, at most one fractional cell chosen from the partial sequence, and
/// empty cells, so the painted width is always exactly `width`.
pub fn progress_bar(cx: &mut ComponentContext, props: &Props<ProgressBarProps>) -> Node {
    let theme = cx.use_theme();
    let drag_state = cx.use_ref(ProgressDragState::default);
    let max = (props.max | 100).max(1);
    let value = (props.value | 0).min(max);
    let width = usize::from(props.width | 20);
    let filled_glyph = props.filled.clone() | theme.progress.filled.clone();
    let empty_glyph = props.empty.clone() | theme.progress.empty.clone();
    let partial = props.partial.clone() | theme.progress.partial.clone();
    let percentage = (u128::from(value) * 100 / u128::from(max)) as u64;

    let (full, partial_index, empty) = split_cells(value, max, width, &partial);
    let mut bar_spans = Vec::new();
    if full > 0 {
        bar_spans.push(
            Span::new(filled_glyph.symbol().repeat(full))
                .style(theme.progress.filled_style.clone()),
        );
    }
    if let Some(index) = partial_index
        && let Some(glyph) = partial.symbol(index)
    {
        bar_spans.push(Span::new(glyph.to_string()).style(theme.progress.filled_style.clone()));
    }
    if empty > 0 {
        bar_spans.push(
            Span::new(empty_glyph.symbol().repeat(empty)).style(theme.progress.empty_style.clone()),
        );
    }

    let mut host = props.host_props(DomProps::default());
    host.style.layout /= Layout::Horizontal;

    let label = props.label.clone() | String::new();
    let mut children = Vec::new();
    if !label.is_empty() {
        children.push(
            Text::new(format!("{label} "))
                .text_style(theme.typography.label.clone())
                .into(),
        );
    }
    if width > 0 {
        let mut bar_dom = DomProps::default();
        bar_dom.style.width /= Dimension::Cells(width as u16);
        if props.draggable | false {
            bar_dom.events.pointer_down = Attr::Set(drag_listener(
                drag_state.clone(),
                props.on_drag_start.as_ref().cloned(),
                width,
                max,
                DragEdge::Start,
            ));
            bar_dom.events.pointer_move = Attr::Set(drag_listener(
                drag_state.clone(),
                props.on_drag.as_ref().cloned(),
                width,
                max,
                DragEdge::Move,
            ));
            bar_dom.events.pointer_up = Attr::Set(end_listener(
                drag_state.clone(),
                props.on_drag.as_ref().cloned(),
                props.on_drag_end.as_ref().cloned(),
                width,
                max,
            ));
            bar_dom.events.pointer_cancel = Attr::Set(cancel_listener(
                drag_state.clone(),
                props.on_drag_end.as_ref().cloned(),
                max,
            ));
        }
        children.push(ui! { <view dom={bar_dom}>{Text::from_spans(bar_spans)}</view> });
    }
    if props.show_percentage | true {
        children.push(
            Text::new(format!(" {percentage:>3}%"))
                .text_style(theme.typography.muted.clone())
                .into(),
        );
    }
    let children: Node = children.into_iter().collect();

    ui! { <view dom={host}>{children}</view> }
}

/// Which edge of a drag a listener answers.
#[derive(Clone, Copy)]
enum DragEdge {
    /// The press that begins a drag.
    Start,
    /// Pointer motion while a drag is in progress.
    Move,
}

/// Builds the pointer listener for the press or the motion of a drag.
///
/// The listener is caller-owned: it reports the value under the pointer and
/// leaves the rendered value untouched. A press consumes the event so an
/// ancestor selection or scroll region does not also react, and motion reports
/// only a value that actually changed.
fn drag_listener(
    drag_state: crate::Ref<ProgressDragState>,
    listener: Option<EventListener<ProgressBarDragEvent>>,
    width: usize,
    max: u64,
    edge: DragEdge,
) -> EventListener<PointerEvent> {
    EventListener::new(move |event: PointerEvent| {
        if matches!(edge, DragEdge::Start) && !event.is_primary_button() {
            return;
        }
        let mut state = drag_state.lock().expect("progress drag state poisoned");
        match edge {
            DragEdge::Start => {
                event.stop_propagation();
                let value = drag_value(event.local_position.column, width, max);
                state.dragging = true;
                state.value = value;
                drop(state);
                if let Some(listener) = &listener {
                    listener.call(ProgressBarDragEvent { value, max });
                }
            }
            DragEdge::Move => {
                if !state.dragging {
                    return;
                }
                event.stop_propagation();
                let value = drag_value(event.local_position.column, width, max);
                if value == state.value {
                    return;
                }
                state.value = value;
                drop(state);
                if let Some(listener) = &listener {
                    listener.call(ProgressBarDragEvent { value, max });
                }
            }
        }
    })
}

/// Builds the release listener: it reports a value that changed on the way up
/// through `on_drag` and always reports the final value through `on_drag_end`.
fn end_listener(
    drag_state: crate::Ref<ProgressDragState>,
    changed: Option<EventListener<ProgressBarDragEvent>>,
    listener: Option<EventListener<ProgressBarDragEvent>>,
    width: usize,
    max: u64,
) -> EventListener<PointerEvent> {
    EventListener::new(move |event: PointerEvent| {
        let mut state = drag_state.lock().expect("progress drag state poisoned");
        if !state.dragging {
            return;
        }
        state.dragging = false;
        event.stop_propagation();
        let value = drag_value(event.local_position.column, width, max);
        let moved = value != state.value;
        state.value = value;
        drop(state);
        if moved && let Some(changed) = &changed {
            changed.call(ProgressBarDragEvent { value, max });
        }
        if let Some(listener) = &listener {
            listener.call(ProgressBarDragEvent { value, max });
        }
    })
}

/// Builds the cancellation listener: a cancelled drag still ends, so the
/// caller's own drag state cannot leak.
fn cancel_listener(
    drag_state: crate::Ref<ProgressDragState>,
    listener: Option<EventListener<ProgressBarDragEvent>>,
    max: u64,
) -> EventListener<PointerEvent> {
    EventListener::new(move |_event: PointerEvent| {
        let value = {
            let mut state = drag_state.lock().expect("progress drag state poisoned");
            if !state.dragging {
                return;
            }
            state.dragging = false;
            state.value
        };
        if let Some(listener) = &listener {
            listener.call(ProgressBarDragEvent { value, max });
        }
    })
}

/// Splits `width` cells into full cells, an optional fractional cell index, and
/// empty cells for `value / max`.
///
/// With a non-empty ramp, the fractional index is `ceil(remainder * len / max) - 1`,
/// so element `i` covers the fill band `(i / len, (i + 1) / len]` of the cell.
/// With an empty ramp, the filled count is rounded to the nearest cell, the
/// legacy behavior.
fn split_cells(
    value: u64,
    max: u64,
    width: usize,
    partial: &GlyphRamp,
) -> (usize, Option<usize>, usize) {
    let numerator = u128::from(value) * width as u128;
    let denominator = u128::from(max);
    let whole = (numerator / denominator) as usize;
    let remainder = numerator % denominator;
    if !partial.is_empty() && remainder > 0 {
        let len = partial.len() as u128;
        let index = ((remainder * len).div_ceil(denominator) - 1) as usize;
        let index = index.min(partial.len() - 1);
        let full = whole.min(width);
        (full, Some(index), width.saturating_sub(full + 1))
    } else {
        let filled = ((numerator + denominator / 2) / denominator) as usize;
        let filled = filled.min(width);
        (filled, None, width - filled)
    }
}

/// Maps a bar-relative pointer column to the value at that cell's left edge.
fn drag_value(column: i32, width: usize, max: u64) -> u64 {
    if width == 0 {
        return 0;
    }
    let column = column.clamp(0, width as i32) as u128;
    let value = (column * u128::from(max) + width as u128 / 2) / width as u128;
    value.min(u128::from(max)) as u64
}

/// Configuration for [`spinner`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpinnerProps {
    /// Frame index into the ten-frame animation; defaults to `0` and wraps.
    pub frame: Attr<usize>,
    /// Optional text drawn after the frame; defaults to empty.
    pub label: Attr<String>,
}

/// Single-frame spinner; callers advance `frame` to animate it. See
/// [`SpinnerProps`] for its configuration.
pub fn spinner(cx: &mut ComponentContext, props: &Props<SpinnerProps>) -> Node {
    const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let theme = cx.use_theme();
    let label = props.label.clone() | String::new();
    let mut spans =
        vec![Span::new(FRAMES[(props.frame | 0) % FRAMES.len()]).foreground(theme.colors.primary)];
    if !label.is_empty() {
        spans.push(Span::new(format!(" {label}")).style(theme.typography.body.clone()));
    }
    ui! { <view dom={props.dom.clone()}>{Text::from_spans(spans)}</view> }
}

/// Semantic role that selects a badge's themed colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BadgeVariant {
    /// Primary role.
    #[default]
    Primary,
    /// Secondary role.
    Secondary,
    /// Accent role.
    Accent,
    /// Muted role.
    Muted,
    /// Destructive role.
    Destructive,
}

/// Configuration for [`badge`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BadgeProps {
    /// Badge text, padded with one space on each side; defaults to empty.
    pub text: Attr<String>,
    /// Color role; defaults to [`BadgeVariant::Primary`].
    pub variant: Attr<BadgeVariant>,
}

/// Small solid-background label; see [`BadgeProps`] for its configuration.
pub fn badge(cx: &mut ComponentContext, props: &Props<BadgeProps>) -> Node {
    let theme = cx.use_theme();
    let (background, foreground) = match props.variant | BadgeVariant::Primary {
        BadgeVariant::Primary => (theme.colors.primary, theme.colors.primary_foreground),
        BadgeVariant::Secondary => (theme.colors.secondary, theme.colors.secondary_foreground),
        BadgeVariant::Accent => (theme.colors.accent, theme.colors.accent_foreground),
        BadgeVariant::Muted => (theme.colors.muted, theme.colors.muted_foreground),
        BadgeVariant::Destructive => (
            theme.colors.destructive,
            theme.colors.destructive_foreground,
        ),
    };
    let text = props.text.clone() | String::new();
    ui! {
        <view dom={props.dom.clone()}>{Text::new(format!(" {text} "))
            .foreground(foreground)
            .background(background)
            .bold()}</view>
    }
}

/// Semantic role that selects an alert's accent color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AlertVariant {
    /// Informational accent.
    #[default]
    Info,
    /// Success accent.
    Success,
    /// Warning accent.
    Warning,
    /// Error accent.
    Error,
}

/// Configuration for [`alert`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AlertProps {
    /// Bold title line; defaults to empty.
    pub title: Attr<String>,
    /// Wrapped body message; defaults to empty.
    pub message: Attr<String>,
    /// Accent role; defaults to [`AlertVariant::Info`].
    pub variant: Attr<AlertVariant>,
}

/// Card with a colored left stripe, a bold title, and a softly wrapped
/// message; see [`AlertProps`] for its configuration.
///
/// The stripe keeps the solid role; the title falls back to the card
/// foreground when that role would be unreadable as text on the card.
pub fn alert(cx: &mut ComponentContext, props: &Props<AlertProps>) -> Node {
    let theme = cx.use_theme();
    let accent = match props.variant | AlertVariant::Info {
        AlertVariant::Info => theme.colors.primary,
        AlertVariant::Success => theme.colors.secondary,
        AlertVariant::Warning => theme.colors.accent,
        AlertVariant::Error => theme.colors.destructive,
    };
    let title_color = theme.on_card(accent);
    let title_text = props.title.clone() | String::new();
    let message_text = props.message.clone() | String::new();
    let mut style = crate::Style::default();
    style.layout /= Layout::Vertical;
    style.gap /= theme.spacing.xs;
    style.padding /= Edges {
        top: 0,
        right: 0,
        bottom: 0,
        left: theme.spacing.sm,
    };
    style.background /= theme.colors.card;
    style.text.foreground /= theme.colors.card_foreground;
    style.border.kind /= theme.borders.kind;
    style.border.edges /= Edges {
        top: false,
        right: false,
        bottom: false,
        left: true,
    };
    style.border.foreground /= accent;
    style.border.background /= theme.colors.card;
    let dom = props.host_props(DomProps {
        style,
        ..DomProps::default()
    });
    ui! {
        <view dom={dom}>
            {Text::new(title_text).foreground(title_color).bold()}
            {Text::new(message_text)
                .text_style(theme.typography.body.clone())
                .wrap(crate::TextWrap::Soft)}
        </view>
    }
}

/// Configuration for [`skeleton`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SkeletonProps {
    /// Placeholder width in terminal cells; defaults to `12`.
    pub width: Attr<u16>,
}

/// Muted block of placeholder glyphs; see [`SkeletonProps`] for its
/// configuration.
pub fn skeleton(cx: &mut ComponentContext, props: &Props<SkeletonProps>) -> Node {
    let theme = cx.use_theme();
    ui! {
        <view dom={props.dom.clone()}>{Text::new("░".repeat(usize::from(props.width | 12)))
            .foreground(theme.colors.muted_foreground)
            .background(theme.colors.muted)}</view>
    }
}
