//! Markdown-to-component rendering behind the optional `markdown` feature.

use pulldown_cmark::{BlockQuoteKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use unicode_width::UnicodeWidthStr;

use crate::{
    Align, Attr, Component, ComponentContext, Dimension, DomProps, Edges, Layout, Node, Props,
    SelectionAreaProps, Span, Style, Text, TextStyle, TextWrap, blockquote, code, column, divider,
    heading, paragraph, selection_area,
};

/// Configuration for [`markdown`].
#[derive(Debug, Clone, Default)]
pub struct MarkdownProps {
    /// Markdown source to parse and render. An unset value renders an empty document.
    pub text: Attr<String>,
}

#[derive(Clone)]
struct Document {
    blocks: Vec<Block>,
    footnotes: Vec<(String, Vec<Block>)>,
}

#[derive(Clone)]
enum Block {
    Paragraph(Vec<Inline>),
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    Quote {
        kind: Option<BlockQuoteKind>,
        blocks: Vec<Block>,
    },
    Code(String),
    List {
        ordered: bool,
        start: Option<u64>,
        items: Vec<ListItem>,
    },
    Rule,
    Table {
        alignments: Vec<pulldown_cmark::Alignment>,
        head: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    Html(String),
}

#[derive(Clone)]
struct ListItem {
    task: Option<bool>,
    blocks: Vec<Block>,
}

#[derive(Clone)]
enum Inline {
    Text(String),
    Code(String),
    Break {
        hard: bool,
    },
    Footnote(String),
    Html(String),
    Image {
        alt: Vec<Inline>,
        url: String,
    },
    Styled {
        kind: InlineStyle,
        content: Vec<Inline>,
    },
}

#[derive(Clone, Copy)]
enum InlineStyle {
    Emphasis,
    Strong,
    Strike,
    Link,
    Muted,
}

/// Parses Markdown and returns a selectable component tree.
///
/// Markdown is supplied through [`MarkdownProps::text`]. Children are not an
/// input channel for this component and cause a panic when present.
pub fn markdown(cx: &mut ComponentContext, props: &Props<MarkdownProps>) -> Node {
    assert!(
        props.children.is_empty(),
        "markdown accepts input through MarkdownProps::text; children are unsupported"
    );

    let source = props.text.clone() | String::new();
    let document = cx.use_memo(source.clone(), || parse_document(&source));
    let theme = cx.use_theme();
    let blocks = render_document(&document, &theme);

    let selection = selection_area.apply(Props::with_parts(
        DomProps {
            style: Style {
                gap: Attr::Set(theme.spacing.sm),
                ..Style::default()
            },
            ..DomProps::default()
        },
        blocks,
        SelectionAreaProps::default(),
    ));
    let mut style = Style {
        layout: Attr::Set(Layout::Vertical),
        width: Attr::Set(Dimension::Max),
        gap: Attr::Set(theme.spacing.sm),
        text: theme.typography.body.clone(),
        ..Style::default()
    };
    style.align = Attr::Set(Align::Start);
    let host = props.host_props(DomProps {
        style,
        ..DomProps::default()
    });
    Node::element(host, [selection])
}

fn parse_document(source: &str) -> Document {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_GFM);

    let events = Parser::new_ext(source, options).collect::<Vec<_>>();
    let mut cursor = 0;
    let mut footnotes = Vec::new();
    let blocks = parse_blocks(&events, &mut cursor, None, &mut footnotes);
    Document { blocks, footnotes }
}

fn parse_blocks<'a>(
    events: &[Event<'a>],
    cursor: &mut usize,
    stop: Option<TagEnd>,
    footnotes: &mut Vec<(String, Vec<Block>)>,
) -> Vec<Block> {
    let mut blocks = Vec::new();
    while *cursor < events.len() {
        match events[*cursor].clone() {
            Event::End(end) if Some(end) == stop => {
                *cursor += 1;
                break;
            }
            Event::Start(Tag::Paragraph) => {
                *cursor += 1;
                blocks.push(Block::Paragraph(parse_inlines(
                    events,
                    cursor,
                    TagEnd::Paragraph,
                )));
            }
            Event::Start(Tag::Heading { level, .. }) => {
                *cursor += 1;
                blocks.push(Block::Heading {
                    level: heading_number(level),
                    content: parse_inlines(events, cursor, TagEnd::Heading(level)),
                });
            }
            Event::Start(Tag::BlockQuote(kind)) => {
                *cursor += 1;
                blocks.push(Block::Quote {
                    kind,
                    blocks: parse_blocks(events, cursor, Some(TagEnd::BlockQuote(kind)), footnotes),
                });
            }
            Event::Start(Tag::CodeBlock(_kind)) => {
                *cursor += 1;
                let mut content = String::new();
                while *cursor < events.len() {
                    match events[*cursor].clone() {
                        Event::Code(value) | Event::Text(value) => {
                            content.push_str(&value);
                            *cursor += 1;
                        }
                        Event::End(TagEnd::CodeBlock) => {
                            *cursor += 1;
                            break;
                        }
                        _ => *cursor += 1,
                    }
                }
                blocks.push(Block::Code(content));
            }
            Event::Start(Tag::List(start)) => {
                *cursor += 1;
                blocks.push(parse_list(events, cursor, start, footnotes));
            }
            Event::Rule => {
                *cursor += 1;
                blocks.push(Block::Rule);
            }
            Event::Start(Tag::Table(alignments)) => {
                *cursor += 1;
                blocks.push(parse_table(events, cursor, alignments, footnotes));
            }
            Event::Start(Tag::FootnoteDefinition(label)) => {
                *cursor += 1;
                let blocks_for_note =
                    parse_blocks(events, cursor, Some(TagEnd::FootnoteDefinition), footnotes);
                footnotes.push((label.into_string(), blocks_for_note));
            }
            Event::Html(value) => {
                *cursor += 1;
                blocks.push(Block::Html(value.into_string()));
            }
            Event::Text(value) => {
                *cursor += 1;
                blocks.push(Block::Paragraph(vec![Inline::Text(value.into_string())]));
            }
            Event::End(_)
            | Event::Start(_)
            | Event::Code(_)
            | Event::InlineHtml(_)
            | Event::FootnoteReference(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::TaskListMarker(_)
            | Event::DisplayMath(_)
            | Event::InlineMath(_) => {
                *cursor += 1;
            }
        }
    }
    blocks
}

fn parse_list<'a>(
    events: &[Event<'a>],
    cursor: &mut usize,
    start: Option<u64>,
    footnotes: &mut Vec<(String, Vec<Block>)>,
) -> Block {
    let ordered = start.is_some();
    let mut items = Vec::new();
    while *cursor < events.len() {
        match events[*cursor].clone() {
            Event::Start(Tag::Item) => {
                *cursor += 1;
                let task = match events.get(*cursor).cloned() {
                    Some(Event::TaskListMarker(checked)) => {
                        *cursor += 1;
                        Some(checked)
                    }
                    _ => None,
                };
                let blocks = parse_list_item_blocks(events, cursor, footnotes);
                items.push(ListItem { task, blocks });
            }
            Event::End(TagEnd::List(_)) => {
                *cursor += 1;
                break;
            }
            _ => *cursor += 1,
        }
    }
    Block::List {
        ordered,
        start,
        items,
    }
}

fn parse_list_item_blocks<'a>(
    events: &[Event<'a>],
    cursor: &mut usize,
    footnotes: &mut Vec<(String, Vec<Block>)>,
) -> Vec<Block> {
    let mut blocks = Vec::new();
    while *cursor < events.len() {
        if matches!(events[*cursor], Event::End(TagEnd::Item)) {
            *cursor += 1;
            break;
        }
        if is_inline_event(&events[*cursor]) {
            blocks.push(Block::Paragraph(parse_inlines(
                events,
                cursor,
                TagEnd::Item,
            )));
            if matches!(
                events.get(*cursor),
                Some(Event::Start(Tag::Item)) | Some(Event::End(TagEnd::List(_)))
            ) {
                break;
            }
            continue;
        }
        blocks.extend(parse_blocks(events, cursor, Some(TagEnd::Item), footnotes));
        break;
    }
    blocks
}

fn is_inline_event(event: &Event<'_>) -> bool {
    matches!(
        event,
        Event::Text(_)
            | Event::Code(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::FootnoteReference(_)
            | Event::Html(_)
            | Event::InlineHtml(_)
            | Event::TaskListMarker(_)
            | Event::Start(Tag::Emphasis)
            | Event::Start(Tag::Strong)
            | Event::Start(Tag::Strikethrough)
            | Event::Start(Tag::Link { .. })
            | Event::Start(Tag::Image { .. })
    )
}

fn is_block_start(event: &Event<'_>) -> bool {
    matches!(
        event,
        Event::Start(Tag::Paragraph)
            | Event::Start(Tag::Heading { .. })
            | Event::Start(Tag::BlockQuote(_))
            | Event::Start(Tag::CodeBlock(_))
            | Event::Start(Tag::List(_))
            | Event::Start(Tag::Table(_))
            | Event::Start(Tag::FootnoteDefinition(_))
    )
}

fn parse_table<'a>(
    events: &[Event<'a>],
    cursor: &mut usize,
    alignments: Vec<pulldown_cmark::Alignment>,
    footnotes: &mut Vec<(String, Vec<Block>)>,
) -> Block {
    let mut head = Vec::new();
    let mut rows = Vec::new();
    while *cursor < events.len() {
        match events[*cursor].clone() {
            Event::Start(Tag::TableHead) => {
                *cursor += 1;
                head = parse_table_head(events, cursor);
            }
            Event::Start(Tag::TableRow) => rows.push(parse_table_row(events, cursor)),
            Event::End(TagEnd::Table) => {
                *cursor += 1;
                break;
            }
            Event::Start(Tag::FootnoteDefinition(label)) => {
                *cursor += 1;
                let note =
                    parse_blocks(events, cursor, Some(TagEnd::FootnoteDefinition), footnotes);
                footnotes.push((label.into_string(), note));
            }
            _ => *cursor += 1,
        }
    }
    Block::Table {
        alignments,
        head,
        rows,
    }
}

fn parse_table_head<'a>(events: &[Event<'a>], cursor: &mut usize) -> Vec<Vec<Inline>> {
    let mut cells = Vec::new();
    while *cursor < events.len() {
        match events[*cursor].clone() {
            Event::Start(Tag::TableCell) => {
                *cursor += 1;
                cells.push(parse_inlines(events, cursor, TagEnd::TableCell));
            }
            Event::End(TagEnd::TableHead) => {
                *cursor += 1;
                break;
            }
            _ => *cursor += 1,
        }
    }
    cells
}

fn parse_table_row<'a>(events: &[Event<'a>], cursor: &mut usize) -> Vec<Vec<Inline>> {
    *cursor += 1;
    let mut cells = Vec::new();
    while *cursor < events.len() {
        match events[*cursor].clone() {
            Event::Start(Tag::TableCell) => {
                *cursor += 1;
                cells.push(parse_inlines(events, cursor, TagEnd::TableCell));
            }
            Event::End(TagEnd::TableRow) => {
                *cursor += 1;
                break;
            }
            _ => *cursor += 1,
        }
    }
    cells
}

fn parse_inlines<'a>(events: &[Event<'a>], cursor: &mut usize, stop: TagEnd) -> Vec<Inline> {
    let mut inlines = Vec::new();
    while *cursor < events.len() {
        if is_block_start(&events[*cursor]) {
            break;
        }
        match events[*cursor].clone() {
            Event::End(end) if end == stop => {
                *cursor += 1;
                break;
            }
            Event::Text(value) => {
                *cursor += 1;
                inlines.push(Inline::Text(value.into_string()));
            }
            Event::Code(value) => {
                *cursor += 1;
                inlines.push(Inline::Code(value.into_string()));
            }
            Event::SoftBreak => {
                *cursor += 1;
                inlines.push(Inline::Break { hard: false });
            }
            Event::HardBreak => {
                *cursor += 1;
                inlines.push(Inline::Break { hard: true });
            }
            Event::FootnoteReference(value) => {
                *cursor += 1;
                inlines.push(Inline::Footnote(value.into_string()));
            }
            Event::Html(value) | Event::InlineHtml(value) => {
                *cursor += 1;
                inlines.push(Inline::Html(value.into_string()));
            }
            Event::TaskListMarker(checked) => {
                *cursor += 1;
                inlines.push(Inline::Text(if checked { "[x] " } else { "[ ] " }.into()));
            }
            Event::Start(Tag::Emphasis) => {
                *cursor += 1;
                inlines.push(Inline::Styled {
                    kind: InlineStyle::Emphasis,
                    content: parse_inlines(events, cursor, TagEnd::Emphasis),
                });
            }
            Event::Start(Tag::Strong) => {
                *cursor += 1;
                inlines.push(Inline::Styled {
                    kind: InlineStyle::Strong,
                    content: parse_inlines(events, cursor, TagEnd::Strong),
                });
            }
            Event::Start(Tag::Strikethrough) => {
                *cursor += 1;
                inlines.push(Inline::Styled {
                    kind: InlineStyle::Strike,
                    content: parse_inlines(events, cursor, TagEnd::Strikethrough),
                });
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                *cursor += 1;
                inlines.push(Inline::Styled {
                    kind: InlineStyle::Link,
                    content: parse_inlines(events, cursor, TagEnd::Link),
                });
                let _ = dest_url;
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                *cursor += 1;
                let alt = parse_inlines(events, cursor, TagEnd::Image);
                inlines.push(Inline::Image {
                    alt,
                    url: dest_url.into_string(),
                });
            }
            Event::Start(_) => *cursor += 1,
            Event::End(_) | Event::Rule | Event::DisplayMath(_) | Event::InlineMath(_) => {
                *cursor += 1
            }
        }
    }
    inlines
}

fn heading_number(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn render_document(document: &Document, theme: &crate::theme::Theme) -> Vec<Node> {
    let mut nodes = render_blocks(&document.blocks, theme);
    if !document.footnotes.is_empty() {
        let title = heading.children([Text::new("Footnotes").into()]);
        nodes.push(title);
        for (label, blocks) in &document.footnotes {
            nodes.extend(render_blocks_with_prefix(
                blocks,
                theme,
                Some(format!("[{label}] ")),
            ));
        }
    }
    nodes
}

fn render_blocks(blocks: &[Block], theme: &crate::theme::Theme) -> Vec<Node> {
    render_blocks_with_prefix(blocks, theme, None)
}

fn render_blocks_with_prefix(
    blocks: &[Block],
    theme: &crate::theme::Theme,
    prefix: Option<String>,
) -> Vec<Node> {
    blocks
        .iter()
        .enumerate()
        .map(|(index, block)| {
            render_block(block, theme, if index == 0 { prefix.clone() } else { None })
        })
        .collect()
}

fn render_block(block: &Block, theme: &crate::theme::Theme, prefix: Option<String>) -> Node {
    match block {
        Block::Paragraph(content) => paragraph.children([render_text(
            with_prefix(content, prefix),
            TextWrap::Soft,
            theme,
        )]),
        Block::Heading { level, content } => heading.children([render_text_with_style(
            with_prefix(content, prefix),
            TextWrap::Soft,
            heading_style(*level, theme),
            theme,
        )]),
        Block::Quote { kind, blocks } => {
            let prefix = kind.map(|kind| format!("[{:?}] ", kind).to_uppercase());
            blockquote
                .style(|style| style.gap /= theme.spacing.sm)
                .children(render_blocks_with_prefix(blocks, theme, prefix))
        }
        Block::Code(content) => {
            code.children([Text::new(content.clone()).wrap(TextWrap::NoWrap).into()])
        }
        Block::List {
            ordered,
            start,
            items,
        } => {
            let children = items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let marker = if let Some(checked) = item.task {
                        if checked {
                            "[x]".to_owned()
                        } else {
                            "[ ]".to_owned()
                        }
                    } else if *ordered {
                        format!("{}.", start.unwrap_or(1) + index as u64)
                    } else {
                        "•".to_owned()
                    };
                    let mut rendered =
                        render_blocks_with_prefix(&item.blocks, theme, Some(format!("{marker} ")));
                    if rendered.is_empty() {
                        rendered.push(paragraph.children([Text::new(format!("{marker} ")).into()]));
                    }
                    column
                        .style(|style| {
                            style.padding /= Edges {
                                left: theme.spacing.sm,
                                ..Edges::default()
                            }
                        })
                        .children(rendered)
                })
                .collect::<Vec<_>>();
            column.children(children)
        }
        Block::Rule => divider.node(),
        Block::Table {
            alignments,
            head,
            rows,
        } => render_table(alignments, head, rows, theme),
        Block::Html(content) => {
            code.children([Text::new(content.clone()).wrap(TextWrap::NoWrap).into()])
        }
    }
}

fn with_prefix(content: &[Inline], prefix: Option<String>) -> Vec<Inline> {
    let mut out = Vec::new();
    if let Some(prefix) = prefix {
        out.push(Inline::Styled {
            kind: InlineStyle::Muted,
            content: vec![Inline::Text(prefix)],
        });
    }
    out.extend(content.iter().cloned());
    out
}

fn heading_style(level: u8, theme: &crate::theme::Theme) -> TextStyle {
    let (foreground, bold, dim, italic, underlined, overlined) = match level {
        1 => (theme.colors.primary, true, false, false, true, false),
        2 => (theme.colors.secondary, true, false, false, false, true),
        3 => (theme.colors.accent, true, false, true, false, false),
        4 => (theme.colors.destructive, false, false, true, true, false),
        5 => (
            theme.colors.muted_foreground,
            false,
            true,
            false,
            false,
            true,
        ),
        _ => (theme.colors.foreground, false, true, true, false, false),
    };
    let mut style = TextStyle::default();
    style.foreground /= foreground;
    style.attr.bold /= bold;
    style.attr.dim /= dim;
    style.attr.italic /= italic;
    style.attr.underlined /= underlined;
    style.attr.overlined /= overlined;
    style
}

fn render_text(content: Vec<Inline>, wrap: TextWrap, theme: &crate::theme::Theme) -> Node {
    render_text_with_style(content, wrap, TextStyle::default(), theme)
}

fn render_text_with_style(
    content: Vec<Inline>,
    wrap: TextWrap,
    style: TextStyle,
    theme: &crate::theme::Theme,
) -> Node {
    let mut spans = Vec::new();
    collect_spans(&content, style, theme, &mut spans);
    Text::from_spans(spans).wrap(wrap).into()
}

fn collect_spans(
    content: &[Inline],
    inherited: TextStyle,
    theme: &crate::theme::Theme,
    spans: &mut Vec<Span>,
) {
    for inline in content {
        match inline {
            Inline::Text(value) => spans.push(Span::new(value.clone()).style(inherited.clone())),
            Inline::Code(value) => {
                spans.push(Span::new(value.clone()).style(theme.typography.code.clone()))
            }
            Inline::Break { hard } => {
                spans.push(Span::new(if *hard { "\n" } else { " " }).style(inherited.clone()))
            }
            Inline::Footnote(label) => {
                spans.push(Span::new(format!("[{label}]")).style(inherited.clone()))
            }
            Inline::Html(value) => {
                spans.push(Span::new(value.clone()).style(theme.typography.code.clone()))
            }
            Inline::Image { alt, url } => {
                let mut image = vec![Inline::Text("[image: ".into())];
                image.extend(alt.iter().cloned());
                image.push(Inline::Text(format!("] ({url})")));
                let style = style_for(InlineStyle::Link, &inherited, theme);
                collect_spans(&image, style, theme, spans);
            }
            Inline::Styled { kind, content } => {
                let style = style_for(*kind, &inherited, theme);
                collect_spans(content, style, theme, spans);
            }
        }
    }
}

fn style_for(kind: InlineStyle, inherited: &TextStyle, theme: &crate::theme::Theme) -> TextStyle {
    let mut style = inherited.clone();
    match kind {
        InlineStyle::Emphasis => style.attr.italic /= true,
        InlineStyle::Strong => style.attr.bold /= true,
        InlineStyle::Strike => style.attr.crossed_out /= true,
        InlineStyle::Link => {
            style.foreground /= theme.colors.primary;
            style.attr.underlined /= true;
        }
        InlineStyle::Muted => style = style.with_overrides(&theme.typography.muted),
    }
    style
}

fn render_table(
    alignments: &[pulldown_cmark::Alignment],
    head: &[Vec<Inline>],
    rows: &[Vec<Vec<Inline>>],
    theme: &crate::theme::Theme,
) -> Node {
    let widths = table_widths(alignments.len(), head, rows);
    let mut lines = Vec::new();
    if !head.is_empty() {
        lines.push(render_table_row(head, alignments, &widths, theme, true));
        lines.push(Text::new(table_rule(&widths)).wrap(TextWrap::NoWrap).into());
    }
    lines.extend(
        rows.iter()
            .map(|row| render_table_row(row, alignments, &widths, theme, false)),
    );
    column.style(|style| style.gap /= 0).children(lines)
}

fn render_table_row(
    cells: &[Vec<Inline>],
    alignments: &[pulldown_cmark::Alignment],
    widths: &[usize],
    theme: &crate::theme::Theme,
    header: bool,
) -> Node {
    let mut spans = Vec::new();
    for (index, cell) in cells.iter().enumerate() {
        if index > 0 {
            spans.push(Span::new(" │ "));
        }
        let mut cell_spans = Vec::new();
        collect_spans(cell, TextStyle::default(), theme, &mut cell_spans);
        if header {
            cell_spans = cell_spans.into_iter().map(|span| span.bold()).collect();
        }
        let cell_width = inline_width(cell);
        let extra = widths
            .get(index)
            .copied()
            .unwrap_or(cell_width)
            .saturating_sub(cell_width);
        let (left, right) = match alignments.get(index) {
            Some(pulldown_cmark::Alignment::Right) => (extra, 0),
            Some(pulldown_cmark::Alignment::Center) => (extra / 2, extra - extra / 2),
            _ => (0, extra),
        };
        if left > 0 {
            spans.push(Span::new(" ".repeat(left)));
        }
        spans.extend(cell_spans);
        if right > 0 {
            spans.push(Span::new(" ".repeat(right)));
        }
    }
    Text::from_spans(spans).wrap(TextWrap::NoWrap).into()
}

fn table_widths(columns: usize, head: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>]) -> Vec<usize> {
    let mut widths = vec![1; columns.max(head.len())];
    for row in std::iter::once(head).chain(rows.iter().map(|row| row.as_slice())) {
        for (index, cell) in row.iter().enumerate() {
            if index >= widths.len() {
                widths.push(inline_width(cell));
            } else {
                widths[index] = widths[index].max(inline_width(cell));
            }
        }
    }
    widths
}

fn table_rule(widths: &[usize]) -> String {
    if widths.is_empty() {
        return String::new();
    }
    widths
        .iter()
        .map(|width| "─".repeat((*width).max(1)))
        .collect::<Vec<_>>()
        .join("┼")
}

fn inline_width(content: &[Inline]) -> usize {
    content
        .iter()
        .map(|inline| match inline {
            Inline::Text(value) | Inline::Code(value) | Inline::Html(value) => {
                UnicodeWidthStr::width(value.as_str())
            }
            Inline::Break { hard } => usize::from(*hard),
            Inline::Footnote(label) => UnicodeWidthStr::width(format!("[{label}]").as_str()),
            Inline::Image { alt, url } => {
                UnicodeWidthStr::width("[image: ")
                    + inline_width(alt)
                    + UnicodeWidthStr::width(format!("] ({url})").as_str())
            }
            Inline::Styled { content, .. } => inline_width(content),
        })
        .sum()
}
