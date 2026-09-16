//! Chapter 01 — Start Here.
//!
//! Moves from installation to a useful mental model without assuming prior TUI
//! framework knowledge.

use icmd::{
    Align, BadgeVariant, ButtonVariant, Component, ComponentContext, Dimension, Edges, Justify,
    Layout, Node, Props, Span, Text, TextWrap, badge, button, card, code, column, divider, empty,
    heading, label, muted, paragraph, row, ui, view,
};

use super::{ChapterProps, document, masthead, route_card, section};
use crate::docs::{self, ApiRow, CalloutKind};
use crate::metadata::{self, SectionMeta};
use crate::snippets;

/// A static release-status card composed from ordinary widgets.
fn release_card_demo(_cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    ui! {
        <card style={|style| { style.gap /= 1; }}>
            <row style={|style| {
                style.width /= Dimension::Max;
                style.align /= Align::Center;
                style.justify /= Justify::SpaceBetween;
                style.gap /= 1;
            }}>
                <heading>"icmd 0.1.0"</heading>
                <badge text={"STABLE"} variant={BadgeVariant::Secondary} />
            </row>
            <paragraph>"A retained, terminal-native UI framework with cell-aware layout and Unicode text."</paragraph>
            <divider />
            <row style={|style| { style.gap /= 2; style.align /= Align::Center; }}>
                <muted>"target"</muted>
                <label>"x86_64-unknown-linux-gnu"</label>
            </row>
            <row style={|style| { style.gap /= 1; }}>
                <button on_press={|_| {}}>"Read the guide"</button>
                <button variant={ButtonVariant::Secondary} on_press={|_| {}}>"Release notes"</button>
            </row>
        </card>
    }
}

/// The committed-frame pipeline rendered as a small vertical flow.
fn pipeline_flow(theme: &icmd::theme::Theme) -> Node {
    const STAGES: [(&str, &str); 7] = [
        ("render", "components return nodes"),
        ("lower", "nodes become a DOM"),
        ("layout", "cells are resolved"),
        ("clip", "ancestors bound the paint"),
        ("commit", "one validated frame"),
        ("diff", "changed cells only"),
        ("paint", "the terminal is written"),
    ];
    let primary = theme.colors.primary;
    let muted_foreground = theme.colors.muted_foreground;
    let card = theme.colors.card;
    let border = theme.colors.border;
    let last = STAGES.len() - 1;
    STAGES
        .iter()
        .enumerate()
        .map(|(index, (stage, detail))| {
            ui! {
                <view style={|style| {
                    style.layout /= Layout::Vertical;
                    style.width /= Dimension::Max;
                    style.gap /= 0;
                }}>
                    <row style={move |style| {
                        style.layout /= Layout::Horizontal;
                        style.width /= Dimension::Max;
                        style.gap /= 1;
                        style.padding /= Edges { top: 0, right: 1, bottom: 0, left: 1 };
                        style.background /= card;
                        style.border.kind /= icmd::BorderKind::Single;
                        style.border.foreground /= border;
                        style.border.background /= card;
                    }}>
                        {Text::new(*stage).foreground(primary).bold()}
                        <muted>{Text::new(*detail).wrap(TextWrap::Soft)}</muted>
                    </row>
                    {if index < last {
                        ui! { <muted>{Text::new("↓").foreground(muted_foreground)}</muted> }
                    } else {
                        empty()
                    }}
                </view>
            }
        })
        .collect::<Node>()
}

/// Chapter 01.
pub(super) fn start_here(cx: &mut ComponentContext, props: &Props<ChapterProps>) -> Node {
    let theme = cx.use_theme();
    let data = props.data();
    let meta = metadata::chapter(0);
    let sections: &[SectionMeta] = meta.sections;

    let belongs = section(
        &theme,
        data,
        0,
        &sections[0],
        ui! {
            {docs::body("A terminal interface is not a web page squeezed into cells. It is a grid of characters that a terminal paints, rewrites, and scrolls. icmd keeps that reality visible: components describe intent, the runtime resolves it into cells, and only changed cells reach the terminal.")}
            {docs::body("Four ideas carry most of the framework. Components are retained and return nodes rather than issuing draw calls. Layout is measured in cells, so width is never a guess. Text is shaped by grapheme and cell width, so a CJK glyph or an emoji occupies the columns it really needs. Painting is incremental, so a quiet interface stays quiet.")}
            {docs::live_example(
                &theme,
                "static release card",
                "Every surface below is a built-in widget: card, row, heading, badge, paragraph, divider, and button.",
                ui! { {release_card_demo.apply(())} },
                Some(ui! { {docs::hint(&theme, "notice", "the buttons are real and focusable; this card simply has nothing to do yet")} }),
            )}
            {docs::source_block(&theme, "the same card as a component", snippets::RELEASE_CARD)}
            {docs::notice(&theme, "State ownership: this card owns no state, so it renders the same tree every frame and the runtime paints nothing after the first commit. Interactive controls arrive in chapter 05.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "card", purpose: "bordered surface with card colors", defaults: "vertical layout, xs gap, symmetric padding", events: "none" },
                ApiRow { name: "row", purpose: "horizontal stack", defaults: "small gap", events: "none" },
                ApiRow { name: "badge", purpose: "compact status label", defaults: "BadgeVariant::Primary", events: "none" },
                ApiRow { name: "button", purpose: "semantic activation", defaults: "disabled=false, autofocus=false", events: "on_press" },
            ])}
        },
    );

    let install = section(
        &theme,
        data,
        1,
        &sections[1],
        ui! {
            {docs::body("Add the crate with its defaults when you want native raster support, or disable defaults for a pure-Rust build that only ever emits text cells. Markdown stays opt-in so a small tool does not carry a parser it never uses.")}
            {docs::two_column(
                &theme,
                data.wide(),
                ui! {
                    {docs::feature_row(&theme, "native-raster (default)", "Chafa-backed Kitty, Sixel, and iTerm2 payloads plus symbol rendering. Requires pkg-config, the Chafa development package, and libclang at build time.")}
                    {docs::feature_row(&theme, "markdown (opt-in)", "Turns CommonMark into ordinary, selectable components: headings, lists, tables, task lists, alerts, and footnotes.")}
                    {docs::feature_row(&theme, "no default features", "Cell output only, with no native or parser dependency. Ideal for constrained build environments.")}
                },
                ui! {
                    {docs::source_block(&theme, "Cargo.toml", "[dependencies]\n# Default features: native raster support.\nicmd = \"0.1\"\n\n# Pure-Rust build: cell output only.\nicmd = { version = \"0.1\", default-features = false }\n\n# Documents, with native raster left on.\nicmd = { version = \"0.1\", features = [\"markdown\"] }")}
                },
            )}
            {docs::callout(&theme, CalloutKind::Production, "The documentation browser itself enables both markdown and native-raster, because it demonstrates documents and images. Your application only pays for the features it selects.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "RuntimeConfig", purpose: "session behavior and terminal modes", defaults: "Ctrl+C exit, alternate screen, mouse and paste capture on", events: "none" },
                ApiRow { name: "ImageProtocol", purpose: "which graphics protocol is preferred", defaults: "ImageProtocol::Auto", events: "none" },
            ])}
        },
    );

    let first_component = section(
        &theme,
        data,
        2,
        &sections[2],
        ui! {
            {docs::body("An application is an ordinary component handed to render. The component receives a context and typed props, and returns a Node. No builder, no trait object, and no terminal handle: the runtime owns the terminal.")}
            {docs::source_block(&theme, "complete program", snippets::FIRST_COMPONENT)}
            {docs::notice(&theme, "ComponentContext is the per-render environment: it hands out hooks and context values. Props<()> means this component declares no props, and .apply(()) constructs it. Node is the returned description of the subtree. render installs the terminal session and returns when the session ends.")}
            {docs::watch_for(&theme, "Do not reach for stdout inside a component. The runtime owns the screen; printing from render code corrupts the frame the runtime is about to paint.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "Component", purpose: "trait every component function satisfies", defaults: "—", events: "none" },
                ApiRow { name: "ComponentContext", purpose: "hooks, context, and the app handle", defaults: "fresh per render", events: "none" },
                ApiRow { name: "Props<T>", purpose: "typed payload plus DOM props and children", defaults: "Props::default()", events: "none" },
                ApiRow { name: "render", purpose: "enters the terminal and drives the loop", defaults: "RuntimeConfig::default()", events: "keyboard, pointer, resize" },
            ])}
        },
    );

    let syntax = section(
        &theme,
        data,
        3,
        &sections[3],
        ui! {
            {docs::body("The ui! macro is composition syntax, nothing more. Every tag resolves to an ordinary component call, so a tag you define yourself is written exactly like a built-in one.")}
            {docs::live_example(
                &theme,
                "ui! tour",
                "The buttons below are real; the counter is component state.",
                ui! { {snippet_demo.apply(())} },
                None,
            )}
            {docs::source_block(&theme, "every child form in one tree", snippets::UI_SYNTAX)}
            {docs::notice(&theme, "Text children need no wrapper. {expr} splices any expression that becomes a node. A collection becomes a fragment. Attributes map to typed props, style closures receive a mutable Style, and on_* attributes attach listeners.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "ui!", purpose: "declarative element syntax", defaults: "—", events: "none" },
                ApiRow { name: "fragment", purpose: "group siblings without a box", defaults: "no styling", events: "none" },
                ApiRow { name: "empty", purpose: "render nothing on purpose", defaults: "no layout box", events: "none" },
                ApiRow { name: "Attr<T>", purpose: "tri-state prop value: unset, set, or inherited", defaults: "Attr::Unset", events: "none" },
            ])}
        },
    );

    let frame = section(
        &theme,
        data,
        4,
        &sections[4],
        ui! {
            {docs::body("One frame is a pipeline, and knowing its order explains most surprising behavior. Components render a tree, the runtime lowers it to a DOM, layout resolves every cell, clipping bounds each subtree, and commit produces a validated frame. A diff then turns that frame into cell edits, and the terminal is written once.")}
            {docs::specimen(&theme, "one committed frame", ui! { {pipeline_flow(&theme)} })}
            {docs::body("Two consequences matter while writing an app. First, geometry is only knowable after commit, which is why measurements arrive through element refs. Second, scrolling is clipped by the scroll container, so scroll state is owned by a real host element rather than by your content.")}
            {docs::notice(&theme, "Frame-builder and renderer internals stay in rustdoc. From an application's side, the useful contract is: render purely, let effects own work that outlives a render, and read geometry only from committed snapshots.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "Node", purpose: "a description of one subtree", defaults: "—", events: "none" },
                ApiRow { name: "Frame", purpose: "the validated output of a commit", defaults: "—", events: "none" },
                ApiRow { name: "ElementSnapshot", purpose: "committed geometry and resolved style", defaults: "available after commit", events: "on_element_change" },
            ])}
            {docs::production_note(&theme, "The render path should be quiet: stable tree structure, keyed collections, memoized derived values, and bounded logs. Chapter 09 turns those habits into rules.")}
        },
    );

    let next = section(
        &theme,
        data,
        5,
        &sections[5],
        ui! {
            {docs::body("Pick the passage that matches what you are building. Every chapter starts from a working example and ends with an API strip you can return to.")}
            <view style={|style| {
                style.layout /= Layout::Vertical;
                style.width /= Dimension::Max;
                style.gap /= 1;
            }}>
                {route_card(&theme, data, 1, "◧", "02 · Components and State", "Reusable components, typed props, keys, hooks, effects, and context.")}
                {route_card(&theme, data, 2, "▤", "03 · Layout and Styling", "Boxes, axes, sizing, surfaces, stacking, overflow, and measurement.")}
                {route_card(&theme, data, 3, "¶", "04 · Text and Documents", "Spans, Unicode widths, wrapping, links, selection, and Markdown.")}
                {route_card(&theme, data, 4, "⌘", "05 · Controls and Events", "Buttons, selection controls, editors, focus, and event flow.")}
                {route_card(&theme, data, 5, "◐", "06 · Feedback and Canvas", "Badges, alerts, progress, spinners, and direct cell drawing.")}
                {route_card(&theme, data, 6, "▦", "07 · Scroll, Selection, and Raster Media", "Scroll ownership, selection in scroll, and terminal images.")}
                {route_card(&theme, data, 7, "◆", "08 · Themes", "The nineteen roles, eight presets, derivation, and contrast.")}
                {route_card(&theme, data, 8, "▸", "09 · Production", "Runtime configuration, lifecycle, exit, limits, tests, and release.")}
                {route_card(&theme, data, 9, "≡", "10 · API Map", "Fast lookup for every shipped widget, hook, and type.")}
            </view>
            {docs::callout(&theme, CalloutKind::Info, "Press Ctrl+K at any time to search chapters, sections, widgets, hooks, and types. Ctrl+B collapses the index, and Alt+Arrow moves between chapters and sections.")}
            {docs::hint(&theme, "ctrl+k", "search this guide from anywhere, including while an input has focus")}
            {docs::hint(&theme, "alt+←/→", "previous and next chapter")}
            {docs::hint(&theme, "alt+↑/↓", "previous and next section")}
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
                {belongs}
                {install}
                {first_component}
                {syntax}
                {frame}
                {next}
                {docs::chapter_end(&theme, meta.number, meta.title)}
            </view>
        },
    )
}

/// A small interactive component that mirrors the displayed `ui!` tour.
fn snippet_demo(cx: &mut ComponentContext, _props: &Props<()>) -> Node {
    let (count, set_count) = cx.use_state(|| 0_u32);
    let names = ["alpha", "beta", "gamma"];
    let rows = names
        .iter()
        .map(|name| ui! { <muted>{Text::new(*name)}</muted> })
        .collect::<Node>();
    let increment = set_count.clone();
    let theme = cx.use_theme();
    let primary = theme.colors.primary;
    ui! {
        <column style={|style| {
            style.layout /= Layout::Vertical;
            style.width /= Dimension::Max;
            style.gap /= 1;
        }}>
            {Text::from_spans([
                Span::new("count = ").foreground(primary).bold(),
                Span::new(count.to_string()).bold(),
            ])}
            {rows}
            <row style={|style| { style.gap /= 1; }}>
                <button on_press={move |_| increment.update(|value| *value += 1)}>"Increment"</button>
                <code>"a collection renders as a fragment"</code>
            </row>
        </column>
    }
}
