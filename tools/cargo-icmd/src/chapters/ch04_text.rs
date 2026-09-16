//! Chapter 04 — Text and Documents.
//!
//! The framework's Unicode-aware text model, from role-named components and
//! styled spans through cell widths, wrapping, links, selection, and Markdown.

use icmd::theme::Theme;
use icmd::{
    Align, ButtonVariant, Component, ComponentContext, Dimension, Edges, Layout, Node, Props, Span,
    Text, TextAlign, TextOverflow, TextSelectionEvent, TextWrap, blockquote, button, card, code,
    column, heading, kbd, label, link, markdown, muted, paragraph, row, selection_area, ui, view,
};

use super::{ChapterProps, document, masthead, route_card, section};
use crate::docs::{self, ApiRow, CalloutKind};
use crate::metadata::{self, SectionMeta};
use crate::snippets;

/// One role specimen: the role name in a fixed column, the live component beside it.
fn text_role_row(theme: &Theme, role: &str, body: Node) -> Node {
    let muted_foreground = theme.colors.muted_foreground;
    ui! {
        <row style={|style| {
            style.width /= Dimension::Max;
            style.align /= Align::Start;
            style.gap /= 2;
        }}>
            <view style={|style| { style.width /= Dimension::Cells(11); }}>
                {Text::new(role).foreground(muted_foreground).bold()}
            </view>
            <view style={|style| { style.width /= Dimension::Max; }}>{body}</view>
        </row>
    }
}

/// Every semantic text component, each line naming the role it plays.
fn semantic_text_specimen(theme: &Theme) -> Node {
    ui! {
        <card style={|style| { style.gap /= 0; }}>
            {text_role_row(theme, "heading", ui! { <heading>"Heading level two"</heading> })}
            {text_role_row(theme, "paragraph", ui! { <paragraph>{Text::new("Body copy is the default voice of a document.").wrap(TextWrap::Soft)}</paragraph> })}
            {text_role_row(theme, "label", ui! { <label>"Form label"</label> })}
            {text_role_row(theme, "muted", ui! { <muted>{Text::new("De-emphasized text for secondary detail.").wrap(TextWrap::Soft)}</muted> })}
            {text_role_row(theme, "code", ui! { <code>"cargo icmd docs"</code> })}
            {text_role_row(theme, "blockquote", ui! { <blockquote>{Text::new("Quoted words keep their own voice.").wrap(TextWrap::Soft)}</blockquote> })}
            {text_role_row(theme, "kbd", ui! { <kbd>"Ctrl+K"</kbd> })}
        </card>
    }
}

/// One text node built from independently styled spans, plus a labeled
/// attribute line so each attribute names itself instead of decorating prose.
fn span_specimen(theme: &Theme) -> Node {
    let primary = theme.colors.primary;
    let primary_foreground = theme.colors.primary_foreground;
    let muted_foreground = theme.colors.muted_foreground;
    ui! {
        <card style={|style| { style.gap /= 1; }}>
            {Text::from_spans([
                Span::new("release ").foreground(muted_foreground),
                Span::new("2.0.0").foreground(primary).bold(),
                Span::new(" is "),
                Span::new("gated").italic(),
                Span::new(" on "),
                Span::new("review-42").underlined(),
                Span::new(", so "),
                Span::new("HEAD").reverse(),
                Span::new(" stays put until "),
                Span::new("BLOCKED").foreground(primary_foreground).background(primary).bold(),
                Span::new(" clears."),
            ]).wrap(TextWrap::Soft)}
            {Text::from_spans([
                Span::new("dim: standby").dim(),
                Span::new("   ·   ").foreground(muted_foreground),
                Span::new("crossed_out: superseded").crossed_out(),
                Span::new("   ·   ").foreground(muted_foreground),
                Span::new("reverse: cursor").reverse(),
            ]).wrap(TextWrap::Soft)}
            <muted>{Text::new("One text node, eleven spans, one shaped line.").wrap(TextWrap::Soft)}</muted>
        </card>
    }
}

/// Four scripts in fixed columns: the cell grid, not the byte length, aligns
/// them.
fn unicode_grid(theme: &Theme) -> Node {
    const SAMPLES: [(&str, &str, &str); 4] = [
        ("ascii", "terminal", "8"),
        ("cjk", "端末アプリ", "10"),
        ("combining", "e\u{0301}le\u{0300}ve", "5"),
        ("emoji", "🚀 ship", "7"),
    ];
    let muted_foreground = theme.colors.muted_foreground;
    let rows = SAMPLES
        .iter()
        .map(|(name, sample, cells)| {
            ui! {
                <row style={|style| {
                    style.width /= Dimension::Max;
                    style.gap /= 2;
                }}>
                    <view style={|style| { style.width /= Dimension::Cells(12); }}>
                        {Text::new(*name).foreground(muted_foreground)}
                    </view>
                    <view style={|style| { style.width /= Dimension::Cells(24); }}>
                        {Text::new(*sample).wrap(TextWrap::NoWrap)}
                    </view>
                    <view style={|style| { style.width /= Dimension::Max; }}>
                        <muted>{Text::new(format!("{cells} cells"))}</muted>
                    </view>
                </row>
            }
        })
        .collect::<Node>();
    ui! {
        <card style={|style| { style.gap /= 0; }}>
            <row style={|style| { style.width /= Dimension::Max; style.gap /= 2; }}>
                <view style={|style| { style.width /= Dimension::Cells(12); }}>
                    <muted>"sample"</muted>
                </view>
                <view style={|style| { style.width /= Dimension::Cells(24); }}>
                    <muted>"123456789012345678901234"</muted>
                </view>
                <view style={|style| { style.width /= Dimension::Max; }}>
                    <muted>"width"</muted>
                </view>
            </row>
            {rows}
            <muted>{Text::new(
                "A wide glyph occupies two columns, a combining mark none, and a joined emoji sequence one shaped unit.",
            ).wrap(TextWrap::Soft)}</muted>
        </card>
    }
}

/// Fixed wrap, alignment, and truncation specimens, shown without controls.
fn wrapping_board(theme: &Theme) -> Node {
    const SENTENCE: &str = "the quick brown fox jumps over the lazy dog";
    let muted_foreground = theme.colors.muted_foreground;
    let modes = [
        ("NoWrap", TextWrap::NoWrap),
        ("Soft", TextWrap::Soft),
        ("Hard", TextWrap::Hard),
    ];
    let wrapped = modes
        .iter()
        .map(|(name, wrap)| {
            ui! {
                <view style={|style| {
                    style.layout /= Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.gap /= 0;
                    style.padding /= Edges { top: 0, right: 1, bottom: 0, left: 1 };
                }}>
                    {Text::new(*name).foreground(muted_foreground)}
                    {Text::new(SENTENCE).wrap(*wrap).overflow(TextOverflow::Clip)}
                </view>
            }
        })
        .collect::<Node>();
    ui! {
        <card style={|style| { style.gap /= 1; }}>
            <view style={|style| { style.width /= Dimension::Cells(28); }}>{wrapped}</view>
            <view style={|style| { style.layout /= Layout::Vertical; style.width /= Dimension::Max; style.gap /= 0; }}>
                <muted>"Clip keeps what fits and drops the rest"</muted>
                {Text::new(SENTENCE).wrap(TextWrap::NoWrap).overflow(TextOverflow::Clip)}
                <muted>"Ellipsis marks the truncation with a cell"</muted>
                {Text::new(SENTENCE).wrap(TextWrap::NoWrap).overflow(TextOverflow::Ellipsis)}
                <muted>"Center alignment inside the granted box"</muted>
                {Text::new("centered").align(TextAlign::Center).wrap(TextWrap::NoWrap)}
                <muted>"End alignment inside the granted box"</muted>
                {Text::new("right").align(TextAlign::End).wrap(TextWrap::NoWrap)}
            </view>
        </card>
    }
}

/// Two links and a disabled one; following is recorded, never performed.
fn link_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (last, set_last) = cx.use_state(|| String::from("nothing followed yet"));
    let theme = cx.use_theme();
    let primary = theme.colors.primary;
    let muted_foreground = theme.colors.muted_foreground;
    let follow_changelog = set_last.clone();
    let follow_url = set_last.clone();
    ui! {
        <column style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.gap /= 2;
            }}>
                <link
                    href={"https://example.com/icmd/changelog"}
                    label={"Changelog"}
                    on_follow={move |target: String| follow_changelog.set(target)} />
                <link
                    href={"https://example.com/icmd/migration"}
                    on_follow={move |target: String| follow_url.set(target)} />
            </row>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.gap /= 2;
            }}>
                <link href={"https://example.com/private/runbook"} label={"Internal runbook"} disabled={true} />
                <muted>{Text::new("disabled: muted color and no underline").wrap(TextWrap::Soft)}</muted>
            </row>
            {Text::from_spans([
                Span::new("last follow request: ").foreground(muted_foreground),
                Span::new(last).foreground(primary).bold(),
            ]).wrap(TextWrap::Soft)}
            <muted>{Text::new(
                "Nothing was opened. The link reported its target and this component decided what to record; the guide never launches a browser or a process.",
            ).wrap(TextWrap::Soft)}</muted>
        </column>
    }
}

/// A prose region under `selection_area`, with a byte-range readout.
fn selection_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (report, set_report) = cx.use_state(|| String::from("nothing selected"));
    let theme = cx.use_theme();
    let primary = theme.colors.primary;
    let muted_foreground = theme.colors.muted_foreground;
    let panel = theme.colors.muted;
    ui! {
        <column style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <selection_area
                on_selection_change={move |event: TextSelectionEvent| {
                    set_report.set(format!(
                        "bytes {}..{}  ·  {} characters  ·  {}",
                        event.range.start,
                        event.range.end,
                        event.text.chars().count(),
                        event.text.replace('\n', "⏎"),
                    ));
                }}
                style={move |style| {
                    style.layout /= Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.gap /= 0;
                    style.padding /= Edges::all(1);
                    style.background /= panel;
                }}>
                <paragraph>{Text::new("Cell-aware shaping means the caret lands where the reader expects, even across a wide CJK glyph 端末 or an emoji 🚀.").wrap(TextWrap::Soft)}</paragraph>
                <paragraph>{Text::new("Selection arithmetic runs on the committed frame, so a drag that starts on one of those glyphs and ends on another resolves every cell between them.").wrap(TextWrap::Soft)}</paragraph>
                <muted>{Text::new("Drag across this box, or focus it and extend with Shift+arrows, Ctrl+A to select all, Ctrl+C to copy.").wrap(TextWrap::Soft)}</muted>
            </selection_area>
            {Text::from_spans([
                Span::new("selection: ").foreground(muted_foreground),
                Span::new(report).foreground(primary).bold(),
            ]).wrap(TextWrap::Soft)}
        </column>
    }
}

/// One rich Markdown sample that exercises every block the parser supports.
const MARKDOWN_SAMPLE: &str = r#"# Release checklist

Ship **icmd 0.2.0** only when every gate below is _green_.

> [!NOTE]
> Alert syntax rides on the same GFM extension set your repository already renders.

- [x] unicode width cases
- [ ] raster fallback review

1. Tag the commit
2. Publish the crate

| gate | owner | state |
| --- | --- | --- |
| tests | ci | pass |
| docs | guide | pass |

```rust
let frame = commit()?;
```

A footnote reference anchors the prose.[^limits]

[^limits]: Resource ceilings live in `ResourceLimits`.
"#;

/// A single Source / Rendered toggle over one document.
fn markdown_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (rendered, set_rendered) = cx.use_state(|| true);
    let toggle = set_rendered.clone();
    let theme = cx.use_theme();
    ui! {
        <column style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.gap /= 2;
            }}>
                <button
                    variant={ButtonVariant::Secondary}
                    on_press={move |_| toggle.update(|value| *value = !*value)}>
                    {if rendered { "Show source" } else { "Show rendered" }}
                </button>
                <muted>{Text::new(if rendered { "rendered components" } else { "Markdown source" })}</muted>
            </row>
            {if rendered {
                ui! { <markdown text={MARKDOWN_SAMPLE} /> }
            } else {
                docs::source_block(&theme, "README.md", MARKDOWN_SAMPLE)
            }}
        </column>
    }
}

/// Chapter 04.
pub(super) fn text_and_documents(cx: &mut ComponentContext, props: &Props<ChapterProps>) -> Node {
    let theme = cx.use_theme();
    let data = props.data();
    let meta = metadata::chapter(3);
    let sections: &[SectionMeta] = meta.sections;

    let components = section(
        &theme,
        data,
        0,
        &sections[0],
        ui! {
            {docs::body_pair(
                "Most text in an interface plays a role: a heading states structure, a label names a field, muted copy recedes, code is literal, a quotation is attributed, and a key hint is something a hand presses. Naming the role instead of the appearance is what lets a theme restyle a whole application without touching a component.",
                "Every built-in below is an ordinary component, so a custom one is written the same way and accepts the same style overrides. The theme resolves the role into color, weight, and spacing; the component only declares which role it is.",
            )}
            {docs::specimen(&theme, "one role per line", ui! { {semantic_text_specimen(&theme)} })}
            {docs::notice(&theme, "These components add no colors of their own. `heading`, `label`, `muted`, and `code` read `theme.typography`, and `blockquote` takes its stripe from the accent role, which is why switching themes restyles every one of them at once.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "heading", purpose: "structural text with heading typography", defaults: "no layout change", events: "none" },
                ApiRow { name: "paragraph", purpose: "body copy as a vertical block", defaults: "body typography", events: "none" },
                ApiRow { name: "label", purpose: "field and control labels", defaults: "label typography", events: "none" },
                ApiRow { name: "muted", purpose: "secondary, de-emphasized detail", defaults: "muted typography", events: "none" },
                ApiRow { name: "code", purpose: "literal text on the muted surface", defaults: "code typography, xs/sm padding", events: "none" },
                ApiRow { name: "blockquote", purpose: "attributed quotation", defaults: "muted text, heavy accent left edge", events: "none" },
                ApiRow { name: "kbd", purpose: "keyboard hint", defaults: "label typography on the muted surface", events: "none" },
            ])}
        },
    );

    let spans = section(
        &theme,
        data,
        1,
        &sections[1],
        ui! {
            {docs::body_pair(
                "`Text` is a list of `Span`s. Each span carries the text it contributes and a `TextStyle`, so foreground, background, and every terminal attribute are decided per run while the line is still shaped, wrapped, and measured as one unit.",
                "Build a mixed line with `Text::from_spans`, or set one style for the whole node with the builder methods and override it on individual spans. `TextStyle` is the value type both paths share, so a style derived from the theme and a style written by hand are interchangeable.",
            )}
            {docs::specimen(&theme, "a restrained mixed line", ui! { {span_specimen(&theme)} })}
            {docs::callout(&theme, CalloutKind::Info, "The builder set is the terminal's attribute vocabulary: bold, dim, italic, underlined, crossed_out, reverse, slow_blink, rapid_blink, hidden, fraktur, framed, encircled, and overlined, plus foreground and background. A terminal that cannot paint one simply ignores it, so text never disappears.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "Text", purpose: "styled, shaped, wrappable text node", defaults: "one default span, NoWrap", events: "none" },
                ApiRow { name: "Text::from_spans", purpose: "compose one node from many runs", defaults: "styles start default", events: "none" },
                ApiRow { name: "Span", purpose: "one run: content plus a text style", defaults: "default TextStyle", events: "none" },
                ApiRow { name: "TextStyle", purpose: "foreground, background, and attributes", defaults: "all unset", events: "none" },
                ApiRow { name: "Text::text_style", purpose: "replace the node-level style", defaults: "TextStyle::default()", events: "none" },
            ])}
        },
    );

    let unicode = section(
        &theme,
        data,
        2,
        &sections[2],
        ui! {
            {docs::body_pair(
                "A terminal is a grid of cells, and text is placed in cells rather than bytes or even codepoints. Grapheme shaping decides which codepoints belong to one readable character, then each cluster is measured for width: ASCII and most Latin letters take one column, CJK and most emoji take two, combining marks take none, and a joined emoji sequence is treated as a single unit.",
                "That is why the framework never asks you to count characters. Fixed columns below are filled by different scripts, and the grid itself proves the widths: no padding row is doing the aligning work.",
            )}
            {docs::specimen(&theme, "four scripts, one grid", ui! { {unicode_grid(&theme)} })}
            {docs::source_block(&theme, "the same alignment as a component", snippets::UNICODE_ROWS)}
            {docs::callout(&theme, CalloutKind::Production, "`EmojiMerging` and `RuntimeConfig::emoji_merging` decide whether a joined sequence paints as one unit or as its separate parts. The default merges, which is right for real terminals; `Separate` exists for the rare terminal or test that needs the decomposition. Measure geometry from the committed frame, never from `str::len`.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "Text", purpose: "shapes and measures in cells", defaults: "grapheme clusters", events: "none" },
                ApiRow { name: "Span", purpose: "a run whose width is also summed in cells", defaults: "—", events: "none" },
                ApiRow { name: "EmojiMerging", purpose: "merge or split emoji sequences", defaults: "EmojiMerging::Auto", events: "none" },
                ApiRow { name: "RuntimeConfig", purpose: "session-wide width policy", defaults: "merging, cell output", events: "none" },
            ])}
        },
    );

    let wrapping = section(
        &theme,
        data,
        3,
        &sections[3],
        ui! {
            {docs::body_pair(
                "Three choices decide how a line fits its box. `TextWrap::NoWrap` keeps the line intact and lets the box clip it. `TextWrap::Soft` reflows at word boundaries. `TextWrap::Hard` breaks at the exact column, splitting a word when it has to. `TextAlign` then places each resulting line, and `TextOverflow` says what happens to anything that still does not fit.",
                "The specimens below are fixed so you can compare them side by side without operating anything. Notice that truncation is a property of the text node, not of the container: the same sentence clips in one row and earns an ellipsis in the next.",
            )}
            {docs::specimen(&theme, "wrap, align, and overflow", ui! { {wrapping_board(&theme)} })}
            {docs::source_block(&theme, "the specimens as code", snippets::WRAPPING_SPECIMENS)}
            {docs::notice(&theme, "Hard wrapping is the right default inside a source block, where preserving column alignment matters more than keeping words whole; Soft is the right default for prose. An ellipsis is the non-color cue that content was dropped, so pair it with a way to see the whole value.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "TextWrap", purpose: "NoWrap, Soft, or Hard reflow", defaults: "TextWrap::NoWrap", events: "none" },
                ApiRow { name: "TextAlign", purpose: "Start, Center, or End within the box", defaults: "TextAlign::Start", events: "none" },
                ApiRow { name: "TextOverflow", purpose: "Clip or Ellipsis on the last row", defaults: "TextOverflow::Clip", events: "none" },
                ApiRow { name: "Text::wrap", purpose: "choose the reflow policy", defaults: "NoWrap", events: "none" },
                ApiRow { name: "Text::overflow", purpose: "choose the truncation policy", defaults: "Clip", events: "none" },
            ])}
        },
    );

    let links = section(
        &theme,
        data,
        4,
        &sections[4],
        ui! {
            {docs::body_pair(
                "A link is an inline interactive element with the link treatment: the accent role plus an underline. Activating it, by pointer or by Enter or Space while it holds focus, reports its target through `on_follow`. That is the whole contract.",
                "The framework never opens a browser, launches a process, or touches the network. Following a target is an application decision, so the application is where the policy lives: validate a scheme, copy the URL, open a viewer, or ignore the request entirely.",
            )}
            {docs::live_example(
                &theme,
                "links report, the app decides",
                "Click a link to follow it, or press Enter or Space while it holds focus. The readout records the target; nothing external happens.",
                ui! { {link_demo.apply(())} },
                Some(ui! { {docs::hint(&theme, "a disabled link", "keeps its place in the layout but loses the accent color and the underline")} }),
            )}
            {docs::watch_for(&theme, "Never follow an untrusted `href` blindly. A TUI often runs with a user's full environment, so validate the scheme before handing a target to an opener, and keep the parse failure visible in the interface instead of silently discarding the activation.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "link", purpose: "inline interactive target", defaults: "focusable, not disabled", events: "on_follow" },
                ApiRow { name: "LinkProps", purpose: "href, label, disabled, autofocus", defaults: "label falls back to href", events: "on_follow" },
                ApiRow { name: "href", purpose: "the target reported on activation", defaults: "empty", events: "none" },
                ApiRow { name: "label", purpose: "visible text when no children are given", defaults: "href is shown", events: "none" },
            ])}
        },
    );

    let selection = section(
        &theme,
        data,
        5,
        &sections[5],
        ui! {
            {docs::body_pair(
                "A `selection_area` makes the text below it selectable. The component owns no selection arithmetic: it describes its painted text as the shared selection engine's document and delegates placement, motion, hit testing, and copy to that engine, the same one the editor controls use.",
                "Pointer drag selects. Once the region has focus, arrows move the caret and Shift+arrows extend the selection, `Ctrl+A` selects everything, and `Ctrl+C` copies. The reported range is in document bytes: the region's text leaves concatenated in paint order.",
            )}
            {docs::live_example(
                &theme,
                "select, extend, copy",
                "Drag across the shaded box, then focus it and extend with Shift+arrows. The readout reports the byte range the engine produced.",
                ui! { {selection_demo.apply(())} },
                None,
            )}
            {docs::callout(&theme, CalloutKind::Info, "The active and inactive selection styles come from the theme through `SelectionStyles::from_theme`, so a selectable document and a focused editor present the same selection. `selection_style` and `selection_inactive_style` override either one, and keyboard selection is opt-in per region with `enable_keyboard`, which also gates whether `Ctrl+C` is answered there.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "selection_area", purpose: "make a subtree selectable", defaults: "keyboard selection on, focusable", events: "on_selection_change, on_clipboard" },
                ApiRow { name: "SelectionAreaProps", purpose: "styles, disabled, autofocus, enable_keyboard", defaults: "theme selection styles", events: "on_selection_change" },
                ApiRow { name: "TextSelectionEvent", purpose: "byte range plus the sliced text", defaults: "—", events: "none" },
                ApiRow { name: "TextClipboardEvent", purpose: "copy or cut with the involved text", defaults: "—", events: "on_clipboard" },
                ApiRow { name: "SelectionStyles", purpose: "active and inactive styles from the theme", defaults: "derived from theme colors", events: "none" },
            ])}
        },
    );

    let markdown = section(
        &theme,
        data,
        6,
        &sections[6],
        ui! {
            {docs::body_pair(
                "The `markdown` feature turns CommonMark into ordinary components: headings, paragraphs, emphasis, lists, task lists, tables, alerts, fenced code, and footnotes all become the same nodes this chapter has already introduced. It is opt-in, so a small tool never carries a parser it does not use.",
                "Supply the source through `text`. The parsed document is memoized by source, the output is wrapped in a selectable region, and every block is styled by the active theme rather than by a Markdown stylesheet.",
            )}
            {docs::live_example(
                &theme,
                "one document, two views",
                "The single toggle below switches between the rendered components and the source that produced them.",
                ui! { {markdown_demo.apply(())} },
                Some(ui! { {docs::hint(&theme, "selectable", "rendered Markdown sits in a selection area, so drag, Shift+arrows, Ctrl+A, and Ctrl+C all work on it")} }),
            )}
            {docs::callout(&theme, CalloutKind::Production, "Enable the `markdown` feature deliberately. Parsing happens once per distinct source and is memoized, so a document that never changes costs one parse for the life of the mount; a source rebuilt every frame would re-parse, which is why the sample owns a constant instead of interpolating state.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "markdown", purpose: "CommonMark rendered as components", defaults: "empty document", events: "none" },
                ApiRow { name: "MarkdownProps", purpose: "the `text` source; children are unsupported", defaults: "empty text", events: "none" },
                ApiRow { name: "selection_area", purpose: "wraps the rendered document", defaults: "keyboard selection on", events: "on_selection_change" },
                ApiRow { name: "\"markdown\" feature", purpose: "opt-in parser and renderer", defaults: "disabled", events: "none" },
            ])}
            <view style={|style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                {route_card(&theme, data, 4, "⌘", "05 · Controls and Events", "Buttons, selection controls, editors, focus, keyboard routing, and the event ledger.")}
            </view>
        },
    );

    let background = theme.colors.background;
    document(
        data,
        ui! {
            <view style={move |style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 0;
                style.background /= background;
            }}>
                {masthead(&theme, meta, data.section_count())}
                {components}
                {spans}
                {unicode}
                {wrapping}
                {links}
                {selection}
                {markdown}
                {docs::chapter_end(&theme, meta.number, meta.title)}
            </view>
        },
    )
}
