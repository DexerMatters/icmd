use std::{error::Error, fmt, sync::Arc};

use crossterm::style::{Attributes, Color};
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    Attr, Cell, CellEdit, CellError, DomProps, Edges, Image, ImageError, ImagePosition, Layout,
    Node, PointerButton, PointerEvent, Props, ScrollEvent, Span, Text, WheelEvent,
    basic::{ComponentContext, EventListener, Ref, StateSetter, view},
    theme::Theme,
    ui,
};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProgressBarProps {
    pub value: Attr<u64>,
    pub max: Attr<u64>,
    pub width: Attr<u16>,
    pub show_percentage: Attr<bool>,
    pub label: Attr<String>,
}

pub fn progress_bar(cx: &mut ComponentContext, props: &Props<ProgressBarProps>) -> Node {
    let theme = cx.use_theme();
    let max = (props.max | 100).max(1);
    let value = (props.value | 0).min(max);
    let width = usize::from(props.width | 20);
    let filled =
        ((u128::from(value) * width as u128 + u128::from(max) / 2) / u128::from(max)) as usize;
    let percentage = (u128::from(value) * 100 / u128::from(max)) as u64;

    let label = props.label.clone() | String::new();
    let mut spans = Vec::new();
    if !label.is_empty() {
        spans.push(Span::new(format!("{label} ")).style(theme.typography.label.clone()));
    }
    if filled > 0 {
        spans.push(Span::new("█".repeat(filled)).foreground(theme.colors.primary));
    }
    if filled < width {
        spans.push(Span::new("░".repeat(width - filled)).foreground(theme.colors.muted));
    }
    if props.show_percentage | true {
        spans.push(Span::new(format!(" {percentage:>3}%")).style(theme.typography.muted.clone()));
    }
    ui! { <view dom={props.dom.clone()}>{Text::from_spans(spans)}</view> }
}

pub use progress_bar as progressbar;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ScrollbarOrientation {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScrollbarProps {
    /// Current scroll offset. When set, update it from `on_scroll` to keep the thumb controlled.
    pub offset: Attr<u64>,
    pub content_len: Attr<u64>,
    pub viewport_len: Attr<u64>,
    pub length: Attr<u16>,
    pub orientation: Attr<ScrollbarOrientation>,
    pub enable_mouse: Attr<bool>,
    pub enable_wheel: Attr<bool>,
}

pub fn scrollbar(cx: &mut ComponentContext, props: &Props<ScrollbarProps>) -> Node {
    let theme = cx.use_theme();
    let length = usize::from(props.length | 8);
    let orientation = props.orientation | ScrollbarOrientation::Vertical;
    let vertical = orientation == ScrollbarOrientation::Vertical;
    let content_len = (props.content_len | 1).max(1);
    let viewport_len = (props.viewport_len | 1).min(content_len);
    let max_offset = content_len.saturating_sub(viewport_len);
    let controlled_offset = props.offset.as_ref().copied();
    let (internal_offset, set_internal_offset) =
        cx.use_state(|| controlled_offset.unwrap_or_default().min(max_offset));
    let offset_ref = cx.use_ref(|| controlled_offset.unwrap_or(internal_offset).min(max_offset));
    let drag_grab = cx.use_ref(|| None::<i32>);
    let current_offset = controlled_offset.unwrap_or(internal_offset).min(max_offset);
    *offset_ref.lock().expect("scrollbar offset ref poisoned") = current_offset;

    let thumb_len = if viewport_len >= content_len {
        length
    } else {
        (viewport_len as u128 * length as u128).div_ceil(content_len as u128) as usize
    }
    .clamp(1, length.max(1));
    let max_start = length.saturating_sub(thumb_len);
    let thumb_start = if max_offset == 0 {
        0
    } else {
        ((current_offset * max_start as u64) + max_offset / 2)
            .checked_div(max_offset)
            .unwrap_or_default() as usize
    };
    let enable_mouse = props.enable_mouse | true;
    let wheel_enabled = props.enable_wheel | true;

    let mut dom = props.dom.clone();
    let scroll_listener = props.dom.events.scroll.clone();
    let user_pointer_down = props.dom.events.pointer_down.clone();
    let drag_grab_down = drag_grab.clone();
    let setter_down = set_internal_offset.clone();
    let offset_ref_down = offset_ref.clone();
    let scroll_down = scroll_listener.clone();
    dom.events.pointer_down /= EventListener::new(move |event: PointerEvent| {
        if enable_mouse && event.is_primary_button() {
            let coordinate = scrollbar_coordinate(event.local_position, vertical);
            if coordinate >= thumb_start as i32
                && coordinate < thumb_start.saturating_add(thumb_len) as i32
            {
                *drag_grab_down.lock().expect("scrollbar drag ref poisoned") =
                    Some(coordinate.saturating_sub(thumb_start as i32));
            } else {
                *drag_grab_down.lock().expect("scrollbar drag ref poisoned") = None;
                let next = scrollbar_offset_from_track(coordinate, length, thumb_len, max_offset);
                update_scrollbar_offset(
                    &setter_down,
                    &offset_ref_down,
                    &scroll_down,
                    next,
                    max_offset,
                    vertical,
                );
            }
        }
        call_listener(&user_pointer_down, event);
    });

    let user_pointer_move = props.dom.events.pointer_move.clone();
    let drag_grab_move = drag_grab.clone();
    let setter_move = set_internal_offset.clone();
    let offset_ref_move = offset_ref.clone();
    let scroll_move = scroll_listener.clone();
    dom.events.pointer_move /= EventListener::new(move |event: PointerEvent| {
        if enable_mouse
            && event.buttons & PointerButton::Primary.bit() != 0
            && let Some(grab) = *drag_grab_move.lock().expect("scrollbar drag ref poisoned")
        {
            let coordinate = scrollbar_coordinate(event.local_position, vertical);
            let travel = length.saturating_sub(thumb_len) as i32;
            if travel > 0 && max_offset > 0 {
                let start = coordinate.saturating_sub(grab).clamp(0, travel);
                let next = (start as u64 * max_offset + travel as u64 / 2)
                    .checked_div(travel as u64)
                    .unwrap_or_default();
                update_scrollbar_offset(
                    &setter_move,
                    &offset_ref_move,
                    &scroll_move,
                    next,
                    max_offset,
                    vertical,
                );
            }
        }
        call_listener(&user_pointer_move, event);
    });

    let user_pointer_up = props.dom.events.pointer_up.clone();
    let drag_grab_up = drag_grab.clone();
    dom.events.pointer_up /= EventListener::new(move |event: PointerEvent| {
        *drag_grab_up.lock().expect("scrollbar drag ref poisoned") = None;
        call_listener(&user_pointer_up, event);
    });
    let user_pointer_cancel = props.dom.events.pointer_cancel.clone();
    let drag_grab_cancel = drag_grab.clone();
    dom.events.pointer_cancel /= EventListener::new(move |event: PointerEvent| {
        *drag_grab_cancel
            .lock()
            .expect("scrollbar drag ref poisoned") = None;
        call_listener(&user_pointer_cancel, event);
    });

    let user_wheel = props.dom.events.wheel.clone();
    let setter_wheel = set_internal_offset.clone();
    let offset_ref_wheel = offset_ref.clone();
    let scroll_wheel = scroll_listener.clone();
    dom.events.wheel /= EventListener::new(move |event: WheelEvent| {
        if enable_mouse && wheel_enabled {
            let delta = if vertical {
                i32::from(event.delta_y)
            } else if event.delta_x != 0 {
                i32::from(event.delta_x)
            } else {
                i32::from(event.delta_y)
            };
            let current = *offset_ref_wheel
                .lock()
                .expect("scrollbar offset ref poisoned");
            let next = current
                .saturating_add_signed(i64::from(delta))
                .clamp(0, max_offset);
            update_scrollbar_offset(
                &setter_wheel,
                &offset_ref_wheel,
                &scroll_wheel,
                next,
                max_offset,
                vertical,
            );
        }
        call_listener(&user_wheel, event);
    });

    if length == 0 {
        return ui! { <view dom={dom}>{Text::new("")}</view> };
    }

    let (track, thumb, separator) = match orientation {
        ScrollbarOrientation::Vertical => ("│", "┃", "\n"),
        ScrollbarOrientation::Horizontal => ("─", "━", ""),
    };
    let mut spans = Vec::with_capacity(length * 2);
    for index in 0..length {
        let in_thumb = (thumb_start..thumb_start + thumb_len).contains(&index);
        spans.push(
            Span::new(if in_thumb { thumb } else { track }).foreground(if in_thumb {
                theme.colors.primary
            } else {
                theme.colors.muted_foreground
            }),
        );
        if index + 1 < length && !separator.is_empty() {
            spans.push(Span::new(separator));
        }
    }
    ui! { <view dom={dom}>{Text::from_spans(spans)}</view> }
}

fn scrollbar_coordinate(position: crate::ScreenPosition, vertical: bool) -> i32 {
    if vertical {
        position.line
    } else {
        position.column
    }
}

fn scrollbar_offset_from_track(
    coordinate: i32,
    length: usize,
    thumb_len: usize,
    max_offset: u64,
) -> u64 {
    let travel = length.saturating_sub(thumb_len) as i32;
    if travel <= 0 || max_offset == 0 {
        return 0;
    }
    let start = coordinate
        .saturating_sub(thumb_len as i32 / 2)
        .clamp(0, travel);
    ((start as u64 * max_offset + travel as u64 / 2) / travel as u64).min(max_offset)
}

fn update_scrollbar_offset(
    setter: &StateSetter<u64>,
    offset_ref: &Ref<u64>,
    listener: &Attr<EventListener<ScrollEvent>>,
    next: u64,
    max_offset: u64,
    vertical: bool,
) {
    let next = next.min(max_offset);
    let mut current = offset_ref.lock().expect("scrollbar offset ref poisoned");
    if *current == next {
        return;
    }
    let before = *current;
    *current = next;
    setter.set(next);
    let offset = i32::try_from(next).unwrap_or(i32::MAX);
    let max = i32::try_from(max_offset).unwrap_or(i32::MAX);
    let delta = i32::try_from(next).unwrap_or(i32::MAX) - i32::try_from(before).unwrap_or(i32::MAX);
    let event = if vertical {
        ScrollEvent {
            offset_x: 0,
            offset_y: offset,
            max_x: 0,
            max_y: max,
            delta_x: 0,
            delta_y: delta,
        }
    } else {
        ScrollEvent {
            offset_x: offset,
            offset_y: 0,
            max_x: max,
            max_y: 0,
            delta_x: delta,
            delta_y: 0,
        }
    };
    call_listener(listener, event);
}

fn call_listener<T: Clone>(slot: &Attr<EventListener<T>>, event: T) {
    let listener: Option<EventListener<T>> = slot.clone().into();
    if let Some(listener) = listener {
        listener.call(event);
    }
}

pub type ScrollBarProps = ScrollbarProps;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SpinnerProps {
    pub frame: Attr<usize>,
    pub label: Attr<String>,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BadgeVariant {
    #[default]
    Primary,
    Secondary,
    Accent,
    Muted,
    Destructive,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BadgeProps {
    pub text: Attr<String>,
    pub variant: Attr<BadgeVariant>,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AlertVariant {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AlertProps {
    pub title: Attr<String>,
    pub message: Attr<String>,
    pub variant: Attr<AlertVariant>,
}

pub fn alert(cx: &mut ComponentContext, props: &Props<AlertProps>) -> Node {
    let theme = cx.use_theme();
    let accent = match props.variant | AlertVariant::Info {
        AlertVariant::Info => theme.colors.primary,
        AlertVariant::Success => theme.colors.secondary,
        AlertVariant::Warning => theme.colors.accent,
        AlertVariant::Error => theme.colors.destructive,
    };
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
            {Text::new(title_text).foreground(accent).bold()}
            {Text::new(message_text).style(theme.typography.body.clone())}
        </view>
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SkeletonProps {
    pub width: Attr<u16>,
}

pub fn skeleton(cx: &mut ComponentContext, props: &Props<SkeletonProps>) -> Node {
    let theme = cx.use_theme();
    ui! {
        <view dom={props.dom.clone()}>{Text::new("░".repeat(usize::from(props.width | 12)))
            .foreground(theme.colors.muted_foreground)
            .background(theme.colors.muted)}</view>
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CheckboxProps {
    pub checked: Attr<bool>,
    pub label: Attr<String>,
    pub disabled: Attr<bool>,
}

fn selection_control(
    theme: &Theme,
    selected: bool,
    disabled: bool,
    label: String,
    symbols: (&str, &str),
    inactive: Color,
) -> Node {
    let color = if disabled {
        theme.colors.muted_foreground
    } else if selected {
        theme.colors.primary
    } else {
        inactive
    };
    let label_style = if disabled {
        theme.typography.muted.clone()
    } else {
        theme.typography.body.clone()
    };
    ui! {
        {Text::from_spans([
            Span::new(if selected { symbols.0 } else { symbols.1 }).foreground(color),
            Span::new(format!(" {label}")).style(label_style),
        ])}
    }
}

pub fn checkbox(cx: &mut ComponentContext, props: &Props<CheckboxProps>) -> Node {
    let theme = cx.use_theme();
    let child = selection_control(
        &theme,
        props.checked | false,
        props.disabled | false,
        props.label.clone() | String::new(),
        ("☑", "☐"),
        theme.colors.foreground,
    );
    ui! { <view dom={props.dom.clone()}>{child}</view> }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RadioProps {
    pub selected: Attr<bool>,
    pub label: Attr<String>,
    pub disabled: Attr<bool>,
}

pub fn radio(cx: &mut ComponentContext, props: &Props<RadioProps>) -> Node {
    let theme = cx.use_theme();
    let child = selection_control(
        &theme,
        props.selected | false,
        props.disabled | false,
        props.label.clone() | String::new(),
        ("◉", "○"),
        theme.colors.foreground,
    );
    ui! { <view dom={props.dom.clone()}>{child}</view> }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SwitchProps {
    pub on: Attr<bool>,
    pub label: Attr<String>,
    pub disabled: Attr<bool>,
}

pub fn switch(cx: &mut ComponentContext, props: &Props<SwitchProps>) -> Node {
    let theme = cx.use_theme();
    let child = selection_control(
        &theme,
        props.on | false,
        props.disabled | false,
        props.label.clone() | String::new(),
        ("━●", "●━"),
        theme.colors.muted_foreground,
    );
    ui! { <view dom={props.dom.clone()}>{child}</view> }
}

#[derive(Debug)]
pub enum CanvasError {
    Cell(CellError),
    Image(ImageError),
}

impl fmt::Display for CanvasError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cell(error) => write!(f, "invalid canvas cell: {error}"),
            Self::Image(error) => write!(f, "canvas image error: {error}"),
        }
    }
}

impl Error for CanvasError {}

impl From<CellError> for CanvasError {
    fn from(value: CellError) -> Self {
        Self::Cell(value)
    }
}

impl From<ImageError> for CanvasError {
    fn from(value: ImageError) -> Self {
        Self::Image(value)
    }
}

pub struct CanvasContext {
    image: Image,
    foreground: Color,
    background: Color,
    attributes: Attributes,
}

impl CanvasContext {
    fn try_new(width: u16, height: u16, theme: &Theme) -> Result<Self, CanvasError> {
        let blank = Cell::styled(
            theme.colors.foreground,
            theme.colors.background,
            Attributes::default(),
            " ",
        )
        .expect("a space is a valid canvas cell");
        Ok(Self {
            image: Image::blank(usize::from(width), usize::from(height), blank)?,
            foreground: theme.colors.foreground,
            background: theme.colors.background,
            attributes: Attributes::default(),
        })
    }

    pub fn width(&self) -> usize {
        self.image.width()
    }

    pub fn height(&self) -> usize {
        self.image.height()
    }

    pub fn foreground(&mut self, color: Color) {
        self.foreground = color;
    }

    pub fn background(&mut self, color: Color) {
        self.background = color;
    }

    pub fn attributes(&mut self, attributes: Attributes) {
        self.attributes = attributes;
    }

    pub fn set(&mut self, x: i32, y: i32, symbol: impl Into<String>) -> Result<(), CanvasError> {
        let cell = Cell::new(symbol, self.foreground, self.background, self.attributes)?;
        if x < 0 || y < 0 || x as usize >= self.width() || y as usize >= self.height() {
            return Ok(());
        }
        if cell.width() == 2 && x as usize + 1 >= self.width() {
            return Ok(());
        }
        self.image.patch_cells(&[CellEdit {
            position: ImagePosition::new(y as usize, x as usize),
            cell,
        }])?;
        Ok(())
    }

    pub fn clear(&mut self) -> Result<(), CanvasError> {
        let blank = Cell::styled(self.foreground, self.background, self.attributes, " ")?;
        self.image = Image::blank(self.width(), self.height(), blank)?;
        Ok(())
    }

    pub fn fill_rect(
        &mut self,
        x: i32,
        y: i32,
        width: u16,
        height: u16,
        symbol: impl Into<String>,
    ) -> Result<(), CanvasError> {
        let symbol = symbol.into();
        let cell_width = Cell::new(
            symbol.clone(),
            self.foreground,
            self.background,
            self.attributes,
        )?
        .width() as i32;
        let left = x.max(0);
        let top = y.max(0);
        let right = x.saturating_add(i32::from(width)).min(self.width() as i32);
        let bottom = y
            .saturating_add(i32::from(height))
            .min(self.height() as i32);
        if left >= right || top >= bottom {
            return Ok(());
        }
        for row in top..bottom {
            let mut column = left;
            while column.saturating_add(cell_width) <= right {
                self.set(column, row, symbol.clone())?;
                column = column.saturating_add(cell_width);
                if column >= right {
                    break;
                }
            }
        }
        Ok(())
    }

    pub fn stroke_rect(
        &mut self,
        x: i32,
        y: i32,
        width: u16,
        height: u16,
    ) -> Result<(), CanvasError> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        let right = x.saturating_add(i32::from(width)).saturating_sub(1);
        let bottom = y.saturating_add(i32::from(height)).saturating_sub(1);
        self.set(x, y, "┌")?;
        self.set(right, y, "┐")?;
        self.set(x, bottom, "└")?;
        self.set(right, bottom, "┘")?;
        for column in x.saturating_add(1)..right {
            self.set(column, y, "─")?;
            self.set(column, bottom, "─")?;
        }
        for row in y.saturating_add(1)..bottom {
            self.set(x, row, "│")?;
            self.set(right, row, "│")?;
        }
        Ok(())
    }

    pub fn line(
        &mut self,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        symbol: impl Into<String>,
    ) -> Result<(), CanvasError> {
        let symbol = symbol.into();
        Cell::new(
            symbol.clone(),
            self.foreground,
            self.background,
            self.attributes,
        )?;
        let Some((mut x0, mut y0, x1, y1)) = clip_line(
            i64::from(x0),
            i64::from(y0),
            i64::from(x1),
            i64::from(y1),
            self.width() as i64,
            self.height() as i64,
        ) else {
            return Ok(());
        };
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut error = dx + dy;
        loop {
            self.set(x0 as i32, y0 as i32, symbol.clone())?;
            if x0 == x1 && y0 == y1 {
                break;
            }
            let twice = error.saturating_mul(2);
            if twice >= dy {
                error = error.saturating_add(dy);
                x0 += sx;
            }
            if twice <= dx {
                error = error.saturating_add(dx);
                y0 += sy;
            }
        }
        Ok(())
    }

    pub fn fill_text(&mut self, text: &str, x: i32, y: i32) -> Result<(), CanvasError> {
        let mut column = x;
        let mut row = y;
        for grapheme in text.graphemes(true) {
            if grapheme == "\n" {
                row = row.saturating_add(1);
                column = x;
                continue;
            }
            let (symbol, width) =
                match Cell::new(grapheme, self.foreground, self.background, self.attributes) {
                    Ok(cell) => (grapheme, cell.width() as i32),
                    Err(_) => ("�", 1),
                };
            self.set(column, row, symbol)?;
            column = column.saturating_add(width);
        }
        Ok(())
    }

    fn finish(self) -> Image {
        self.image
    }
}

fn clip_line(
    mut x0: i64,
    mut y0: i64,
    mut x1: i64,
    mut y1: i64,
    width: i64,
    height: i64,
) -> Option<(i64, i64, i64, i64)> {
    if width <= 0 || height <= 0 {
        return None;
    }
    let code = |x: i64, y: i64| {
        (if x < 0 {
            1
        } else if x >= width {
            2
        } else {
            0
        }) | (if y < 0 {
            4
        } else if y >= height {
            8
        } else {
            0
        })
    };
    loop {
        let first = code(x0, y0);
        let second = code(x1, y1);
        if first | second == 0 {
            return Some((x0, y0, x1, y1));
        }
        if first & second != 0 {
            return None;
        }
        let outside = if first != 0 { first } else { second };
        let (nx, ny) = if outside & 8 != 0 {
            let y = height - 1;
            let dy = y1 - y0;
            let x = if dy == 0 {
                x0
            } else {
                x0 + ((x1 - x0) * (y - y0)) / dy
            };
            (x, y)
        } else if outside & 4 != 0 {
            let y = 0;
            let dy = y1 - y0;
            let x = if dy == 0 {
                x0
            } else {
                x0 + ((x1 - x0) * (y - y0)) / dy
            };
            (x, y)
        } else if outside & 2 != 0 {
            let x = width - 1;
            let dx = x1 - x0;
            let y = if dx == 0 {
                y0
            } else {
                y0 + ((y1 - y0) * (x - x0)) / dx
            };
            (x, y)
        } else {
            let x = 0;
            let dx = x1 - x0;
            let y = if dx == 0 {
                y0
            } else {
                y0 + ((y1 - y0) * (x - x0)) / dx
            };
            (x, y)
        };
        if outside == first {
            x0 = nx;
            y0 = ny;
        } else {
            x1 = nx;
            y1 = ny;
        }
    }
}

pub type CanvasDraw = Arc<dyn Fn(&mut CanvasContext) + Send + Sync>;

#[derive(Clone, Default)]
pub struct CanvasProps {
    pub width: Attr<u16>,
    pub height: Attr<u16>,
    pub draw: Attr<CanvasDraw>,
}

impl fmt::Debug for CanvasProps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CanvasProps")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("draw", &"<drawing callback>")
            .finish()
    }
}

pub fn canvas(cx: &mut ComponentContext, props: &Props<CanvasProps>) -> Node {
    let theme = cx.use_theme();
    let width = (props.width | 1).max(1);
    let height = (props.height | 1).max(1);
    let Ok(mut drawing) = CanvasContext::try_new(width, height, &theme) else {
        return ui! { <view dom={props.dom.clone()}>{Text::new("canvas exceeds the supported size")}</view> };
    };
    let draw = props.draw.clone() | Arc::new(|_: &mut CanvasContext| {});
    draw(&mut drawing);
    ui! { <view dom={props.dom.clone()}>{drawing.finish()}</view> }
}
