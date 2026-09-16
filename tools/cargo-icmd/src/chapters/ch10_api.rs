//! Chapter 10 — API Map.
//!
//! Fast lookup without duplicating rustdoc signatures. Every row points at the
//! section that demonstrates the item, and the complete type index is generated
//! from [`crate::metadata`] so it cannot fall behind the registry.

use icmd::{ComponentContext, Dimension, Layout, Node, Props, ui, view};

use super::{ChapterProps, document, masthead, section};
use crate::docs::{self, ApiRow, CalloutKind};
use crate::metadata::{self, SectionMeta};

/// One index entry: the name, what it is for, and where it is demonstrated.
struct Entry {
    /// Widget or type name.
    name: &'static str,
    /// What it is for.
    purpose: &'static str,
}

/// Layout and composition primitives.
const LAYOUT_ENTRIES: [Entry; 13] = [
    Entry {
        name: "view",
        purpose: "The base box: layout, style, events, refs, and focusability.",
    },
    Entry {
        name: "container",
        purpose: "Full-size vertical surface on the theme background.",
    },
    Entry {
        name: "column",
        purpose: "Vertical stack with small spacing.",
    },
    Entry {
        name: "row",
        purpose: "Horizontal stack with small spacing.",
    },
    Entry {
        name: "section",
        purpose: "Full-width vertical group with medium spacing.",
    },
    Entry {
        name: "card",
        purpose: "Bordered, padded surface with the card colors.",
    },
    Entry {
        name: "center",
        purpose: "Row that centers children on both axes.",
    },
    Entry {
        name: "spacer",
        purpose: "Fixed one-cell gap between siblings.",
    },
    Entry {
        name: "footer",
        purpose: "Muted horizontal row for trailing chrome.",
    },
    Entry {
        name: "divider",
        purpose: "One-cell horizontal rule on the top edge.",
    },
    Entry {
        name: "fragment",
        purpose: "Group siblings without introducing a box.",
    },
    Entry {
        name: "empty",
        purpose: "Render nothing on purpose, so a branch can vanish.",
    },
    Entry {
        name: "text",
        purpose: "Raw text leaf when no semantic role fits.",
    },
];

/// Interactive controls and status widgets.
const CONTROL_ENTRIES: [Entry; 15] = [
    Entry {
        name: "button",
        purpose: "Semantic activation with primary, secondary, and destructive roles; emits on_press.",
    },
    Entry {
        name: "checkbox",
        purpose: "Controlled boolean row; requests a toggle through on_change.",
    },
    Entry {
        name: "radio",
        purpose: "Controlled exclusive choice; requests selection through on_select.",
    },
    Entry {
        name: "switch",
        purpose: "Controlled on/off row; requests a change through on_change.",
    },
    Entry {
        name: "input",
        purpose: "Single-line editor, controlled or uncontrolled; emits on_change and on_submit.",
    },
    Entry {
        name: "textarea",
        purpose: "Multi-line editor with wrapping and selection; emits on_change.",
    },
    Entry {
        name: "raw_input",
        purpose: "The editor engine itself, for custom fields that control policy and appearance.",
    },
    Entry {
        name: "link",
        purpose: "Inline hyperlink that reports its target through on_follow and never opens it.",
    },
    Entry {
        name: "scroll_area",
        purpose: "Clipping scroll container that may own or receive its offset; emits on_scroll.",
    },
    Entry {
        name: "selection_area",
        purpose: "Selectable text region over committed content; emits on_selection_change and on_clipboard.",
    },
    Entry {
        name: "badge",
        purpose: "Compact status label whose text always carries the meaning.",
    },
    Entry {
        name: "alert",
        purpose: "Card with a semantic left stripe, a title, and a message.",
    },
    Entry {
        name: "progress_bar",
        purpose: "Completion display driven entirely by the caller's value.",
    },
    Entry {
        name: "spinner",
        purpose: "Single animation frame the caller advances.",
    },
    Entry {
        name: "skeleton",
        purpose: "Muted placeholder block shown while content is pending.",
    },
];

/// Text, document, and media widgets.
const MEDIA_ENTRIES: [Entry; 10] = [
    Entry {
        name: "heading",
        purpose: "Heading-level text using the heading typography token.",
    },
    Entry {
        name: "paragraph",
        purpose: "Body text laid out as a vertical block.",
    },
    Entry {
        name: "label",
        purpose: "Control label text.",
    },
    Entry {
        name: "muted",
        purpose: "De-emphasized secondary text.",
    },
    Entry {
        name: "code",
        purpose: "Inline code on the muted background.",
    },
    Entry {
        name: "blockquote",
        purpose: "Muted block with an accent left border.",
    },
    Entry {
        name: "kbd",
        purpose: "Keyboard-key token on the muted background.",
    },
    Entry {
        name: "canvas",
        purpose: "Fixed cell grid painted by a drawing callback.",
    },
    Entry {
        name: "raster_image",
        purpose: "Raster image placed in an explicit cell box.",
    },
    Entry {
        name: "markdown",
        purpose: "CommonMark rendered as ordinary selectable components.",
    },
];

/// Hooks and context accessors, in selection order.
const HOOK_ENTRIES: [Entry; 12] = [
    Entry {
        name: "use_state",
        purpose: "Visible state plus a cloneable setter that queues updates.",
    },
    Entry {
        name: "use_ref",
        purpose: "Persistent cell that survives renders without causing one.",
    },
    Entry {
        name: "use_state_ref",
        purpose: "The same storage with an encapsulated, poison-reporting lock.",
    },
    Entry {
        name: "use_memo",
        purpose: "Cache derived work; recomputes only when dependencies differ.",
    },
    Entry {
        name: "use_effect",
        purpose: "Run after commit when dependencies change; returns its cleanup.",
    },
    Entry {
        name: "use_mount_effect",
        purpose: "Run once at mount; the same as use_effect with unit dependencies.",
    },
    Entry {
        name: "use_unmount",
        purpose: "Run once at unmount, with no body at mount.",
    },
    Entry {
        name: "use_context",
        purpose: "Read a value published above, or the key's default.",
    },
    Entry {
        name: "provide",
        purpose: "Publish a value to this component's subtree.",
    },
    Entry {
        name: "use_theme",
        purpose: "Read the nearest provided theme.",
    },
    Entry {
        name: "use_element_ref",
        purpose: "Read a host element's committed geometry and resolved style.",
    },
    Entry {
        name: "use_handle",
        purpose: "A cloneable session control handle for graceful exit.",
    },
];

/// Text, document, selection, and raster types.
const MEDIA_TYPES: [Entry; 21] = [
    Entry {
        name: "Text",
        purpose: "Styled spans plus wrap, align, overflow, and layout style.",
    },
    Entry {
        name: "Span",
        purpose: "One styled run inside a Text node.",
    },
    Entry {
        name: "TextStyle",
        purpose: "Foreground, background, and terminal attributes.",
    },
    Entry {
        name: "Attributes",
        purpose: "Bold, dim, italic, underline, reverse, and the rest.",
    },
    Entry {
        name: "TextWrap",
        purpose: "NoWrap, Soft, or Hard.",
    },
    Entry {
        name: "TextAlign",
        purpose: "Start, Center, or End inside the text box.",
    },
    Entry {
        name: "TextOverflow",
        purpose: "Clip at the edge or mark truncation with an ellipsis.",
    },
    Entry {
        name: "SelectionAreaProps",
        purpose: "Selection styling, keyboard opt-in, and clipboard listeners.",
    },
    Entry {
        name: "TextSelectionEvent",
        purpose: "The byte range and text of a committed selection.",
    },
    Entry {
        name: "MarkdownProps",
        purpose: "The Markdown source handed to the widget.",
    },
    Entry {
        name: "CanvasContext",
        purpose: "The mutable cell buffer a drawing callback fills.",
    },
    Entry {
        name: "CanvasProps",
        purpose: "Fixed width, height, and the drawing callback.",
    },
    Entry {
        name: "CanvasDraw",
        purpose: "The shared drawing closure type.",
    },
    Entry {
        name: "CanvasError",
        purpose: "A rejected glyph, image, or raster placement.",
    },
    Entry {
        name: "ImageProps",
        purpose: "Source, requested size, fit, alignment, mode, loading, and an alt label.",
    },
    Entry {
        name: "ImageSource",
        purpose: "An already-loaded image or a file path resolved on demand.",
    },
    Entry {
        name: "ImageFit",
        purpose: "Contain, Cover, or Stretch.",
    },
    Entry {
        name: "ImageMode",
        purpose: "Auto, Native, or Symbols.",
    },
    Entry {
        name: "ImageAlign",
        purpose: "Start, Center, or End on either axis.",
    },
    Entry {
        name: "ImageLoading",
        purpose: "Lazy or eager decoding for file-backed sources.",
    },
    Entry {
        name: "RasterImage",
        purpose: "Decoded RGBA pixels with a stable identity for caching.",
    },
];

/// Runtime and lifecycle types.
const RUNTIME_ENTRIES: [Entry; 12] = [
    Entry {
        name: "render",
        purpose: "Enter the terminal and drive the event loop.",
    },
    Entry {
        name: "render_with",
        purpose: "The same, with an AppLifecycle attached.",
    },
    Entry {
        name: "RuntimeConfig",
        purpose: "Exit key, polling, terminal modes, protocols, and limits.",
    },
    Entry {
        name: "ImageProtocol",
        purpose: "Which graphics protocol a raster surface prefers.",
    },
    Entry {
        name: "ImageUpdatePolicy",
        purpose: "Whether a native repaint is required or merely preferred.",
    },
    Entry {
        name: "ResourceLimits",
        purpose: "Hard ceilings validated before any thread or terminal mode exists.",
    },
    Entry {
        name: "EmojiMerging",
        purpose: "How emoji sequences are merged during layout and rendering.",
    },
    Entry {
        name: "AppLifecycle",
        purpose: "Ordered hooks for each session phase.",
    },
    Entry {
        name: "AppPhase",
        purpose: "Boot, Mount, Ready, Unmount, or Exit.",
    },
    Entry {
        name: "AppSession",
        purpose: "Read-only facts handed to a lifecycle hook.",
    },
    Entry {
        name: "ExitReason",
        purpose: "Why a session ended.",
    },
    Entry {
        name: "AppHandle",
        purpose: "Request a graceful exit and get a clean teardown.",
    },
];

/// Where a name is demonstrated, as a section number such as `3.7`.
fn pointer(name: &str) -> &'static str {
    metadata::covering_section(name)
        .map(|section| section.number)
        .unwrap_or("—")
}

/// Renders a table as compact reference rows, each pointing at its section.
fn entries_table(theme: &icmd::theme::Theme, entries: &[Entry]) -> Node {
    entries
        .iter()
        .map(|entry| docs::ref_row(theme, entry.name, entry.purpose, pointer(entry.name)))
        .collect::<Node>()
}

/// The complete high-level type index, generated from the registry.
fn complete_type_index(theme: &icmd::theme::Theme) -> Node {
    metadata::HIGH_LEVEL_TYPES
        .iter()
        .map(|name| {
            let purpose = metadata::covering_section(name)
                .map(|section| section.title)
                .unwrap_or("undocumented");
            docs::ref_row(theme, name, purpose, pointer(name))
        })
        .collect::<Node>()
}

/// How many widgets the index covers, shown as a headline count.
fn widget_count() -> usize {
    LAYOUT_ENTRIES.len() + CONTROL_ENTRIES.len() + MEDIA_ENTRIES.len()
}

/// Chapter 10.
pub(super) fn api_map(cx: &mut ComponentContext, props: &Props<ChapterProps>) -> Node {
    let theme = cx.use_theme();
    let data = props.data();
    let meta = metadata::chapter(9);
    let sections: &[SectionMeta] = meta.sections;

    let layout = section(
        &theme,
        data,
        0,
        &sections[0],
        ui! {
            {docs::body("Every layout primitive in one place, with the section that demonstrates it. The pointer is a section number: `3.1` means chapter 03, section 1.")}
            {docs::specimen(&theme, "composition and layout", ui! {
                {entries_table(&theme, &LAYOUT_ENTRIES)}
            })}
            {docs::notice(&theme, "These are the only boxes. Everything larger is one of them nested inside another, which is why the vocabulary stays small enough to hold in your head.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "Layout", purpose: "Vertical, Horizontal, or Absolute", defaults: "Vertical", events: "none" },
                ApiRow { name: "Dimension", purpose: "Auto, Cells, Percent, or Max", defaults: "Auto", events: "none" },
                ApiRow { name: "Edges", purpose: "Per-edge margin, padding, and border flags", defaults: "all zero or false", events: "none" },
            ])}
        },
    );

    let controls = section(
        &theme,
        data,
        1,
        &sections[1],
        ui! {
            {docs::body("Every widget, with its ownership model in the purpose text: controls that own nothing render what you pass and request a change, while the container-like widgets own their own scroll or selection state and report it.")}
            {docs::specimen(&theme, "controls and feedback", ui! {
                {entries_table(&theme, &CONTROL_ENTRIES)}
            })}
            {docs::watch_for(&theme, "`on_press` is the semantic activation event for a button, not a raw click. It fires for pointer activation and for Enter or Space on the focused control, so a keyboard-only user gets the same behavior without a second code path.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "ButtonProps", purpose: "disabled, autofocus, variant, on_press", defaults: "variant is Primary", events: "on_press" },
                ApiRow { name: "InputProps", purpose: "value, default_value, placeholder, max_length, and listeners", defaults: "uncontrolled when value is unset", events: "on_change, on_submit, on_clipboard" },
                ApiRow { name: "ScrollAreaProps", purpose: "axes, offset, wheel step, scrollbar visibility", defaults: "vertical axis, auto scrollbar", events: "on_scroll" },
            ])}
        },
    );

    let hooks = section(
        &theme,
        data,
        2,
        &sections[2],
        ui! {
            {docs::body("Pick a hook by the question you are answering. Visible value that changes the UI is state. A value that must survive a render without causing one is a ref. Work derived from other values is a memo. Work that leaves the render is an effect. Anything shared across a subtree is context.")}
            {docs::specimen(&theme, "hooks and context", ui! {
                {entries_table(&theme, &HOOK_ENTRIES)}
            })}
            {docs::watch_for(&theme, "Hooks are matched by position. Two hooks may not swap places between renders, and a hook may not appear inside a conditional, because the second render would read the wrong slot.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "StateSetter", purpose: "queue an update and wake the runtime", defaults: "cloneable, not Copy", events: "none" },
                ApiRow { name: "StateRef", purpose: "encapsulated shared mutable state", defaults: "reports poisoning as StateError", events: "none" },
                ApiRow { name: "ContextKey", purpose: "typed key naming a provided value", defaults: "carries its own default", events: "none" },
            ])}
        },
    );

    let media = section(
        &theme,
        data,
        3,
        &sections[3],
        ui! {
            {docs::body("The text, document, selection, and raster type map. Widget entries come first, then the types you name when you build content by hand.")}
            {docs::specimen(&theme, "text, documents, and media widgets", ui! {
                {entries_table(&theme, &MEDIA_ENTRIES)}
            })}
            {docs::specimen(&theme, "content type map", ui! {
                {entries_table(&theme, &MEDIA_TYPES)}
            })}
            {docs::notice(&theme, "Text is measured in cells, not bytes. A wide CJK glyph occupies two columns and a combining mark none, so alignment is a property of the shaped text rather than of the string.")}
            {docs::api_strip(&theme, &[
                ApiRow { name: "Text", purpose: "spans plus wrap, align, overflow", defaults: "NoWrap, Start, Clip", events: "none" },
                ApiRow { name: "ImageSource", purpose: "loaded pixels or a file path", defaults: "lazy for files", events: "none" },
                ApiRow { name: "markdown", purpose: "CommonMark as native components", defaults: "requires the markdown feature", events: "none" },
            ])}
        },
    );

    let runtime = section(
        &theme,
        data,
        4,
        &sections[4],
        ui! {
            {docs::body("Configuration, lifecycle, and the boundary of this guide. Everything above is the application author's surface; the frame builder, renderer, and pipeline live in `icmd::advanced` and in rustdoc, where their protocols can be described precisely.")}
            {docs::specimen(&theme, "runtime and lifecycle", ui! {
                {entries_table(&theme, &RUNTIME_ENTRIES)}
            })}
            {docs::callout(&theme, CalloutKind::Info, "Reach for `icmd::advanced` only when you are building your own pipeline or frame. An ordinary application configures `RuntimeConfig`, attaches an `AppLifecycle` with `render_with`, and never names a pipeline stage.")}
            {docs::body("Finally, the complete documented type index. It is generated from the same registry that drives search, so every entry here is searchable from `Ctrl+K` and every widget exported by `icmd::widgets` appears above.")}
            {docs::live_example(
                &theme,
                "complete documented type index",
                "Every high-level hook and type this guide covers, with the section that demonstrates it.",
                ui! {
                    <view style={|style| {
                        style.layout /= Layout::Vertical;
                        style.width /= Dimension::Max;
                        style.gap /= 0;
                    }}>
                        {complete_type_index(&theme)}
                    </view>
                },
                Some(ui! { {docs::hint(&theme, "coverage", &format!("{} widgets and {} documented types", widget_count(), metadata::HIGH_LEVEL_TYPES.len()))} }),
            )}
            {docs::api_strip(&theme, &[
                ApiRow { name: "icmd::widgets", purpose: "every built-in widget constructor", defaults: "one flat namespace", events: "none" },
                ApiRow { name: "icmd::events", purpose: "live event types plus raw terminal events", defaults: "none", events: "all families" },
                ApiRow { name: "icmd::advanced", purpose: "pipeline protocol and runtime ownership", defaults: "for custom pipelines only", events: "none" },
            ])}
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
                {layout}
                {controls}
                {hooks}
                {media}
                {runtime}
                {docs::chapter_end(&theme, meta.number, meta.title)}
            </view>
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widget_index_covers_every_exported_widget() {
        let listed: Vec<&str> = LAYOUT_ENTRIES
            .iter()
            .chain(CONTROL_ENTRIES.iter())
            .chain(MEDIA_ENTRIES.iter())
            .map(|entry| entry.name)
            .collect();
        for widget in metadata::WIDGETS {
            assert!(
                listed.contains(widget),
                "the API map must list the `{widget}` widget"
            );
        }
        assert_eq!(
            listed.len(),
            metadata::WIDGETS.len(),
            "the API map must list every widget exactly once"
        );
    }

    #[test]
    fn every_index_row_points_at_a_real_section() {
        for name in metadata::WIDGETS
            .iter()
            .chain(metadata::HIGH_LEVEL_TYPES.iter())
        {
            let section = metadata::covering_section(name)
                .unwrap_or_else(|| panic!("`{name}` must be covered by a section"));
            assert!(
                !section.number.is_empty() && !section.title.is_empty(),
                "`{name}` must point at a labeled section"
            );
        }
    }

    #[test]
    fn hook_and_type_tables_cover_the_documented_set() {
        let hooks: Vec<&str> = HOOK_ENTRIES.iter().map(|entry| entry.name).collect();
        for hook in [
            "use_state",
            "use_ref",
            "use_state_ref",
            "use_memo",
            "use_effect",
            "use_mount_effect",
            "use_unmount",
            "use_context",
            "provide",
            "use_theme",
            "use_element_ref",
            "use_handle",
        ] {
            assert!(hooks.contains(&hook), "the hook index must list `{hook}`");
        }
        let types: Vec<&str> = MEDIA_TYPES
            .iter()
            .chain(RUNTIME_ENTRIES.iter())
            .map(|entry| entry.name)
            .collect();
        for name in [
            "Text",
            "Span",
            "CanvasContext",
            "RasterImage",
            "RuntimeConfig",
            "AppLifecycle",
        ] {
            assert!(types.contains(&name), "the type map must list `{name}`");
        }
    }

    #[test]
    fn every_registry_widget_has_a_pointer() {
        for widget in metadata::WIDGETS {
            assert_ne!(pointer(widget), "—", "`{widget}` must resolve to a section");
        }
    }
}
